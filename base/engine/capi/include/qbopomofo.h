/*
 * QBopomofo composing session C API
 *
 * Manages mixed Chinese/English composing on top of the chewing context.
 * Used by both macOS (Swift) and Windows (Rust) platforms.
 */

#ifndef qbopomofo_composing_h
#define qbopomofo_composing_h

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Opaque handle for a QBopomofo composing session. */
typedef struct QBComposingSession QBComposingSession;

/** Shift behavior constants. */
#define QB_SHIFT_NONE 0
#define QB_SHIFT_SMART_TOGGLE 1
#define QB_SHIFT_TOGGLE_ONLY 2

/** Create a new ComposingSession with default Q注音 preferences. */
QBComposingSession *qb_composing_new(void);

/** Delete a ComposingSession. */
void qb_composing_delete(QBComposingSession *session);

/** Check if currently in English mode. Returns 1 if English, 0 if Chinese. */
int qb_composing_is_english(const QBComposingSession *session);

/** Check if there is mixed content (segments recorded). */
int qb_composing_has_mixed_content(const QBComposingSession *session);

/**
 * Handle Shift key press/release.
 * @param is_down 1 = pressed, 0 = released.
 * @param chinese_buffer current chewing buffer content (UTF-8, NULL-terminated).
 * @return 1 if mode changed, 0 if not.
 */
int qb_composing_handle_shift(QBComposingSession *session, int is_down, const char *chinese_buffer);

/** Check if Shift is currently held down. */
int qb_composing_is_shift_held(const QBComposingSession *session);

/** Mark that a key was typed while Shift was held (prevents mode toggle on release). */
void qb_composing_mark_shift_used(QBComposingSession *session);

/**
 * Type an English character.
 * @param chinese_buffer current chewing buffer content (UTF-8). NULL or "" if empty.
 * @return 1 if caller should directly commit this character (no Chinese context),
 *         0 if character was added to mixed composing buffer.
 */
int qb_composing_type_english(QBComposingSession *session, uint8_t ch, const char *chinese_buffer);

/** Delete the last English character. Returns 1 if deleted, 0 if empty. */
int qb_composing_backspace_english(QBComposingSession *session);

/** Get the English buffer content. Caller must free with chewing_free(). */
char *qb_composing_english_buffer(const QBComposingSession *session);

/**
 * Build the full display string from segments + current buffers.
 * @param chinese_buffer current chewing buffer (UTF-8).
 * @param bopomofo current bopomofo reading (UTF-8).
 * @return UTF-8 string. Caller must free with chewing_free().
 */
char *qb_composing_build_display(const QBComposingSession *session, const char *chinese_buffer, const char *bopomofo);

/**
 * Commit all content in correct order.
 * @param final_chinese committed text from chewing_handle_Enter (UTF-8).
 * @return The full committed string. Caller must free with chewing_free().
 */
char *qb_composing_commit_all(QBComposingSession *session, const char *final_chinese);

/** Clear all composing state (Esc/reset). */
void qb_composing_clear(QBComposingSession *session);

/**
 * Insert an English character at a specific display cursor position (mixed content).
 * @param ch ASCII character to insert.
 * @param cursor display cursor position (character index).
 * @param chinese_buffer current chewing buffer (UTF-8).
 * @param bopomofo current bopomofo reading (UTF-8).
 * @return 1 if handled (English region), 0 if not (Chinese/bopomofo region).
 */
int qb_composing_insert_at_cursor(QBComposingSession *session, uint8_t ch, int cursor,
                                   const char *chinese_buffer, const char *bopomofo);

/**
 * Delete the character before the given display cursor position (mixed content).
 * @param cursor display cursor position (character index).
 * @param chinese_buffer current chewing buffer (UTF-8).
 * @param bopomofo current bopomofo reading (UTF-8).
 * @return 0 = nothing, 1 = English char deleted, 2 = Chinese region (delegate to chewing).
 */
int qb_composing_delete_at_cursor(QBComposingSession *session, int cursor,
                                   const char *chinese_buffer, const char *bopomofo);

/**
 * Query the region type at a given display cursor position.
 * @param cursor display cursor position (character index).
 * @param chinese_buffer current chewing buffer (UTF-8).
 * @param bopomofo current bopomofo reading (UTF-8).
 * @return 0=Chinese(segment), 1=English(segment), 2=RemainingChinese,
 *         3=Bopomofo, 4=EnglishBuffer, -1=at/past end
 */
int qb_composing_cursor_region(const QBComposingSession *session, int cursor,
                                const char *chinese_buffer, const char *bopomofo);

/**
 * Convert a display cursor position to the corresponding chewing engine cursor position.
 * @param cursor display cursor position (character index).
 * @param chinese_buffer current chewing buffer (UTF-8).
 * @param bopomofo current bopomofo reading (UTF-8).
 * @return chewing cursor index, or -1 if the position is not in a Chinese region.
 */
int qb_composing_display_to_chewing_cursor(const QBComposingSession *session, int cursor,
                                            const char *chinese_buffer, const char *bopomofo);

/**
 * Re-synchronize Chinese segments after chewing buffer changed (e.g. candidate selection).
 * Call this after chewing_cand_choose_by_index when mixed content is active.
 */
void qb_composing_resync_chinese(QBComposingSession *session, const char *chinese_buffer);

/** Set Shift behavior. Use QB_SHIFT_* constants. */
void qb_composing_set_shift_behavior(QBComposingSession *session, int behavior);

#ifdef __cplusplus
}
#endif

#endif /* qbopomofo_composing_h */
