//! Edit session management for TSF.
//!
//! TSF requires all text operations to happen inside an edit session.
//! The flow is:
//! 1. Create an EditSession with a pending operation
//! 2. Call `context.RequestEditSession(tid, &session, flags)`
//! 3. TSF calls `session.DoEditSession(ec)` with an edit cookie
//! 4. Inside DoEditSession, perform the actual text operations using the cookie

use std::cell::RefCell;
use std::rc::Rc;

use windows::core::{implement, Interface};
use windows::core::BOOL;
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfContextView,
    ITfEditSession, ITfEditSession_Impl, ITfInsertAtSelection, ITfRange,
    TF_ES_READWRITE, TF_ES_SYNC, TF_IAS_QUERYONLY, INSERT_TEXT_AT_SELECTION_FLAGS,
    TF_ST_CORRECTION,
};

/// Operations that can be performed inside an edit session.
pub enum EditOp {
    /// Start or update composition with preedit text
    UpdateComposition {
        text: String,
        composition: Option<ITfComposition>,
        sink: ITfCompositionSink,
    },
    /// Commit text to the application and end composition
    CommitText {
        text: String,
        composition: Option<ITfComposition>,
    },
    /// End the current composition without committing
    EndComposition {
        composition: Option<ITfComposition>,
    },
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
            EditOp::UpdateComposition { text, composition, sink } => {
                crate::qb_dbg!("DoEditSession: UpdateComposition text={:?}", text);
                let new_comp = do_update_composition(ec, &self.context, &text, composition, &sink)?;
                let caret_pos = get_composition_caret(ec, &self.context, &new_comp);
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
                    let _ = unsafe { comp.EndComposition(ec) };
                }
                *self.result.borrow_mut() = Some(EditResult::Composition(None, None));
            }
        }

        Ok(())
    }
}

/// Request a synchronous edit session.
pub fn request_edit_session(
    context: &ITfContext,
    tid: u32,
    op: EditOp,
) -> windows::core::Result<Option<EditResult>> {
    // Shared cell: the session callback writes the result here,
    // and we read it after RequestEditSession returns (sync).
    let result_cell: ResultCell = Rc::new(RefCell::new(None));

    let session = QBEditSession::new(context, op, Rc::clone(&result_cell));
    let session_itf: ITfEditSession = session.into();

    crate::qb_dbg!("request_edit_session: calling RequestEditSession tid={}", tid);

    let _hr = unsafe {
        context.RequestEditSession(
            tid,
            &session_itf,
            TF_ES_READWRITE | TF_ES_SYNC,
        )?
    };

    crate::qb_dbg!("request_edit_session: done");

    // Since TF_ES_SYNC, the callback has already executed.
    // Extract the result from the shared cell.
    Ok(result_cell.borrow_mut().take())
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
        // End composition if text is empty
        if let Some(comp) = composition {
            let _ = unsafe { comp.EndComposition(ec) };
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
        let range = unsafe {
            insert_at_selection.InsertTextAtSelection(
                ec,
                TF_IAS_QUERYONLY,
                &[],
            )?
        };

        // Start a composition on this range
        let context_composition: ITfContextComposition = context.cast()?;
        let comp = unsafe {
            context_composition.StartComposition(ec, &range, sink)?
        };

        // Set the text on the composition range
        let comp_range = unsafe { comp.GetRange()? };
        unsafe { comp_range.SetText(ec, 0, &text_w)? };

        comp
    };

    Ok(Some(comp))
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
        let _ = unsafe { comp.EndComposition(ec) };
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

/// Get the screen coordinates of the composition caret (bottom-left of text extent).
fn get_composition_caret(
    ec: u32,
    context: &ITfContext,
    composition: &Option<ITfComposition>,
) -> Option<(i32, i32)> {
    let comp = composition.as_ref()?;
    let range = unsafe { comp.GetRange().ok()? };

    // Clone range and collapse to end (caret position)
    let caret_range = unsafe { range.Clone().ok()? };
    unsafe { let _ = caret_range.Collapse(ec, windows::Win32::UI::TextServices::TF_ANCHOR_END); }

    let view: ITfContextView = unsafe { context.GetActiveView().ok()? };
    let mut rect = windows::Win32::Foundation::RECT::default();
    let mut clipped = BOOL::default();
    unsafe { view.GetTextExt(ec, &caret_range, &mut rect, &mut clipped).ok()? };

    Some((rect.left, rect.bottom))
}
