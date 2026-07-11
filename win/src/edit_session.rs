//! Edit session management for TSF.
//!
//! TSF requires all text operations to happen inside an edit session.
//! The flow is:
//! 1. Create an EditSession with a pending operation
//! 2. Call `context.RequestEditSession(tid, &session, flags)`
//! 3. TSF calls `session.DoEditSession(ec)` with an edit cookie
//! 4. Inside DoEditSession, perform the actual text operations using the cookie

use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::rc::Rc;

use windows::Win32::Foundation::E_UNEXPECTED;
use windows::Win32::UI::TextServices::{
    INSERT_TEXT_AT_SELECTION_FLAGS, ITfComposition, ITfCompositionSink, ITfContext,
    ITfContextComposition, ITfContextView, ITfEditSession, ITfEditSession_Impl,
    ITfInsertAtSelection, ITfRange, TF_AE_NONE, TF_ANCHOR_END, TF_ANCHOR_START, TF_ES_READWRITE,
    TF_ES_SYNC, TF_IAS_QUERYONLY, TF_SELECTION, TF_SELECTIONSTYLE, TF_ST_CORRECTION,
};
use windows::core::{BOOL, HRESULT, Interface, implement};

/// Operations that can be performed inside an edit session.
pub enum EditOp {
    /// Start or update composition with preedit text
    UpdateComposition {
        text: String,
        cursor_utf16: usize,
        needs_caret_position: bool,
        update_selection: bool,
        composition: Option<ITfComposition>,
        sink: ITfCompositionSink,
    },
    /// Commit text to the application and end composition
    CommitText {
        text: String,
        composition: Option<ITfComposition>,
    },
    /// End the current composition without committing
    EndComposition { composition: Option<ITfComposition> },
}

/// Result from an edit session — the updated composition state.
pub enum EditResult {
    Composition(Option<ITfComposition>, Option<(i32, i32)>),
}

/// Shared cell for passing results out of the edit session callback.
type ResultCell = Rc<RefCell<Option<EditResult>>>;

/// An edit session that performs a single text operation.
#[implement(ITfEditSession)]
pub struct QBEditSession {
    context: ITfContext,
    op: RefCell<Option<EditOp>>,
    result: ResultCell,
}

impl QBEditSession {
    fn new(context: &ITfContext, op: EditOp, result: ResultCell) -> Self {
        Self {
            context: context.clone(),
            op: RefCell::new(Some(op)),
            result,
        }
    }
}

impl ITfEditSession_Impl for QBEditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> windows::core::Result<()> {
        crate::qb_dbg!("DoEditSession: ec={}", ec);

        let op = self.op.borrow_mut().take();
        let Some(op) = op else { return Ok(()) };

        match op {
            EditOp::UpdateComposition {
                text,
                cursor_utf16,
                needs_caret_position,
                update_selection,
                composition,
                sink,
            } => {
                crate::qb_dbg!("DoEditSession: UpdateComposition text={:?}", text);
                let new_comp = do_update_composition(ec, &self.context, &text, composition, &sink)?;
                let text_utf16_len = text.encode_utf16().count();
                let caret_range = if update_selection || needs_caret_position {
                    set_composition_caret(
                        ec,
                        &self.context,
                        &new_comp,
                        cursor_utf16.min(text_utf16_len),
                        update_selection,
                    )
                } else {
                    None
                };
                let caret_pos = if needs_caret_position {
                    caret_range
                        .as_ref()
                        .and_then(|range| get_range_screen_position(ec, &self.context, range))
                } else {
                    None
                };
                *self.result.borrow_mut() = Some(EditResult::Composition(new_comp, caret_pos));
            }
            EditOp::CommitText { text, composition } => {
                crate::qb_dbg!("DoEditSession: CommitText text={:?}", text);
                do_commit_text(ec, &self.context, &text, composition)?;
                *self.result.borrow_mut() = Some(EditResult::Composition(None, None));
            }
            EditOp::EndComposition { composition } => {
                crate::qb_dbg!("DoEditSession: EndComposition");
                if let Some(comp) = composition {
                    clear_and_end_composition(ec, &comp)?;
                }
                *self.result.borrow_mut() = Some(EditResult::Composition(None, None));
            }
        }

        Ok(())
    }
}

/// Threshold above which a TSF edit session is considered "slow" and
/// gets logged to %TEMP%\qbopomofo_slow.log. RequestEditSession with
/// TF_ES_SYNC blocks until the host app releases its document lock —
/// when the host stalls (Electron, Office, some browsers), every
/// keystroke pays that latency. The log lets us tell "our code is slow"
/// from "the host is stalling us".
const SLOW_EDIT_SESSION_MS: u128 = 30;

/// Request a synchronous edit session.
pub fn request_edit_session(
    context: &ITfContext,
    tid: u32,
    op: EditOp,
) -> windows::core::Result<EditResult> {
    // Capture op label before the op is moved into QBEditSession.
    let op_label: &'static str = match &op {
        EditOp::UpdateComposition { .. } => "UpdateComposition",
        EditOp::CommitText { .. } => "CommitText",
        EditOp::EndComposition { .. } => "EndComposition",
    };

    // Shared cell: the session callback writes the result here,
    // and we read it after RequestEditSession returns (sync).
    let result_cell: ResultCell = Rc::new(RefCell::new(None));

    let session = QBEditSession::new(context, op, Rc::clone(&result_cell));
    let session_itf: ITfEditSession = session.into();

    crate::qb_dbg!(
        "request_edit_session: calling RequestEditSession tid={} op={}",
        tid,
        op_label
    );

    let start = std::time::Instant::now();
    let session_hr =
        unsafe { context.RequestEditSession(tid, &session_itf, TF_ES_READWRITE | TF_ES_SYNC)? };
    let elapsed_ms = start.elapsed().as_millis();

    if elapsed_ms >= SLOW_EDIT_SESSION_MS {
        crate::qb_slow!(
            "RequestEditSession({}) took {}ms tid={}",
            op_label,
            elapsed_ms,
            tid,
        );
    }

    crate::qb_dbg!("request_edit_session: done in {}ms", elapsed_ms);

    // RequestEditSession has two HRESULTs: the outer COM return (handled by
    // `?` above) and phrSession, returned by the windows crate as this value.
    // S_OK outside does not mean DoEditSession ran; phrSession can still be
    // TF_E_SYNCHRONOUS, TS_E_READONLY, or the callback's own failure.
    validate_session_result(session_hr)?;

    // Since TF_ES_SYNC succeeded, the callback must have produced a result.
    result_cell
        .borrow_mut()
        .take()
        .ok_or_else(|| windows::core::Error::from(E_UNEXPECTED))
}

fn validate_session_result(hr: HRESULT) -> windows::core::Result<()> {
    hr.ok()
}

// ---------------------------------------------------------------------------
// Edit operations (called inside DoEditSession with valid ec)
// ---------------------------------------------------------------------------

fn do_update_composition(
    ec: u32,
    context: &ITfContext,
    text: &str,
    composition: Option<ITfComposition>,
    sink: &ITfCompositionSink,
) -> windows::core::Result<Option<ITfComposition>> {
    let text_w: Vec<u16> = text.encode_utf16().collect();

    if text.is_empty() {
        // Clear then end composition if text is empty. Ending a composition
        // without clearing its range lets some TSF hosts keep/commit the last
        // preedit character.
        if let Some(comp) = composition {
            clear_and_end_composition(ec, &comp)?;
        }
        return Ok(None);
    }

    let comp = if let Some(comp) = composition {
        // Update existing composition
        let range: ITfRange = unsafe { comp.GetRange()? };
        unsafe { range.SetText(ec, 0, &text_w)? };
        comp
    } else {
        // Start new composition
        let insert_at_selection: ITfInsertAtSelection = context.cast()?;

        // Get range at current selection (query only, don't insert)
        let range =
            unsafe { insert_at_selection.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])? };

        // Start a composition on this range
        let context_composition: ITfContextComposition = context.cast()?;
        let comp = unsafe { context_composition.StartComposition(ec, &range, sink)? };

        // Set the text on the composition range
        let comp_range = unsafe { comp.GetRange()? };
        unsafe { comp_range.SetText(ec, 0, &text_w)? };

        comp
    };

    Ok(Some(comp))
}

fn clear_and_end_composition(ec: u32, comp: &ITfComposition) -> windows::core::Result<()> {
    let range: ITfRange = unsafe { comp.GetRange()? };
    let empty = [0u16];
    unsafe { range.SetText(ec, 0, &empty[..0])? };
    unsafe { comp.EndComposition(ec)? };
    Ok(())
}

fn do_commit_text(
    ec: u32,
    context: &ITfContext,
    text: &str,
    composition: Option<ITfComposition>,
) -> windows::core::Result<()> {
    let text_w: Vec<u16> = text.encode_utf16().collect();

    if let Some(comp) = composition {
        // Set final text on the composition range and end
        let range: ITfRange = unsafe { comp.GetRange()? };
        unsafe { range.SetText(ec, TF_ST_CORRECTION, &text_w)? };
        unsafe { comp.EndComposition(ec)? };
    } else {
        // No active composition — insert directly at selection
        let insert_at_selection: ITfInsertAtSelection = context.cast()?;
        let _range = unsafe {
            insert_at_selection.InsertTextAtSelection(
                ec,
                INSERT_TEXT_AT_SELECTION_FLAGS(0),
                &text_w,
            )?
        };
    }

    Ok(())
}

/// Move the TSF selection to a UTF-16 offset inside the composition and return
/// the collapsed caret range. Selection updates are best-effort because some
/// hosts expose read/write composition text but reject explicit selection.
fn set_composition_caret(
    ec: u32,
    context: &ITfContext,
    composition: &Option<ITfComposition>,
    cursor_utf16: usize,
    update_selection: bool,
) -> Option<ITfRange> {
    let comp = composition.as_ref()?;
    let caret_range = unsafe { comp.GetRange().ok()?.Clone().ok()? };

    unsafe { caret_range.Collapse(ec, TF_ANCHOR_START).ok()? };
    if cursor_utf16 > 0 {
        let mut shifted = 0i32;
        unsafe {
            caret_range
                .ShiftEnd(
                    ec,
                    cursor_utf16.min(i32::MAX as usize) as i32,
                    &mut shifted,
                    std::ptr::null(),
                )
                .ok()?;
            caret_range.Collapse(ec, TF_ANCHOR_END).ok()?;
        }
    }

    if update_selection {
        let mut selection = TF_SELECTION {
            range: ManuallyDrop::new(Some(caret_range.clone())),
            style: TF_SELECTIONSTYLE {
                ase: TF_AE_NONE,
                fInterimChar: BOOL(0),
            },
        };
        let set_result = unsafe { context.SetSelection(ec, std::slice::from_ref(&selection)) };
        unsafe { ManuallyDrop::drop(&mut selection.range) };
        set_result.ok()?;
    }

    Some(caret_range)
}

/// Get the screen coordinates of an already-collapsed composition caret.
fn get_range_screen_position(
    ec: u32,
    context: &ITfContext,
    caret_range: &ITfRange,
) -> Option<(i32, i32)> {
    let view: ITfContextView = unsafe { context.GetActiveView().ok()? };
    let mut rect = windows::Win32::Foundation::RECT::default();
    let mut clipped = BOOL::default();
    unsafe {
        view.GetTextExt(ec, caret_range, &mut rect, &mut clipped)
            .ok()?
    };

    Some((rect.left, rect.bottom))
}

#[cfg(test)]
mod tests {
    use super::validate_session_result;
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::UI::TextServices::TF_E_SYNCHRONOUS;

    #[test]
    fn inner_edit_session_hresult_is_not_ignored() {
        assert!(validate_session_result(S_OK).is_ok());
        assert!(validate_session_result(TF_E_SYNCHRONOUS).is_err());
    }
}
