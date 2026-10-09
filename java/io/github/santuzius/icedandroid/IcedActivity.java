package io.github.santuzius.icedandroid;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.content.res.Configuration;
import android.graphics.Color;
import android.graphics.Rect;
import android.os.Build;
import android.os.Bundle;
import android.text.Editable;
import android.text.InputType;
import android.text.Selection;
import android.view.ActionMode;
import android.view.KeyEvent;
import android.view.Menu;
import android.view.MenuItem;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;
import android.window.OnBackInvokedDispatcher;

import java.text.BreakIterator;

/**
 * NativeActivity runs android_main in the app's Rust library. This subclass adds what the Rust side cannot do through the NDK:
 * the soft keyboard with a real input connection, the text menu, the clipboard, system-bar insets, dark mode and the back gesture.
 *
 * Use it as the app's activity in AndroidManifest.xml, with the library name in the usual {@code android.app.lib_name} meta-data.
 * Methods without "native" are called from Rust (iced_android's src/android.rs) on the iced thread; UI work is posted to the UI thread.
 */
public class IcedActivity extends NativeActivity {

    static native void nativeImeCommit(String text);
    /** 0 = Backspace, 1 = Delete, 2 = Enter, 3 = Left, 4 = Right, 5 = Shift+Right. */
    static native void nativeImeKey(int key);
    /** 0 = cut, 1 = copy, 2 = paste, 3 = select all. */
    static native void nativeShortcut(int action);
    static native void nativeBack();
    static native void nativeInsets(float top, float right, float bottom, float left);
    static native void nativeNightMode(boolean night);
    /** iced's text around the cursor of the focused field as [before, selected, after]; null for password fields. */
    static native String[] nativeTextContext();
    static native boolean nativeDeletesGraphemes();

    private ImeView imeView;
    private ActionMode textMenu;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        // NativeActivity loads the library itself, but only System.loadLibrary makes its JNI functions (native* above) visible to Java.
        System.loadLibrary(libraryName());
        super.onCreate(savedInstanceState);

        imeView = new ImeView(this);
        addContentView(imeView, new ViewGroup.LayoutParams(1, 1));

        drawBehindSystemBars();
        reportNightMode(getResources().getConfiguration());

        if (Build.VERSION.SDK_INT >= 33) {
            // From Android 13 the back gesture no longer arrives as KEYCODE_BACK once enableOnBackInvokedCallback is set; up to 12 winit reports the key itself.
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT, IcedActivity::nativeBack);
        }
    }

    /** The library named in the manifest for NativeActivity, "main" by default as there. */
    private String libraryName() {
        try {
            ActivityInfo info = getPackageManager().getActivityInfo(getIntent().getComponent(), PackageManager.GET_META_DATA);

            if (info.metaData != null && info.metaData.getString(META_DATA_LIB_NAME) != null) {
                return info.metaData.getString(META_DATA_LIB_NAME);
            }
        } catch (PackageManager.NameNotFoundException ignored) {
        }

        return "main";
    }

    @Override
    public void onConfigurationChanged(Configuration config) {
        super.onConfigurationChanged(config);
        reportNightMode(config);
    }

    /** Lets the native surface cover the whole screen and reports the space the bars, cutout and keyboard take, in dp. */
    @SuppressWarnings("deprecation")
    private void drawBehindSystemBars() {
        Window window = getWindow();
        View decor = window.getDecorView();

        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false);
        } else {
            decor.setSystemUiVisibility(View.SYSTEM_UI_FLAG_LAYOUT_STABLE | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
        }

        // Android 15+ makes the bars transparent anyway; earlier versions need it set.
        window.setStatusBarColor(Color.TRANSPARENT);
        window.setNavigationBarColor(Color.TRANSPARENT);

        if (Build.VERSION.SDK_INT >= 29) {
            window.setNavigationBarContrastEnforced(false);
        }

        float density = getResources().getDisplayMetrics().density;

        decor.setOnApplyWindowInsetsListener((view, insets) -> {
            int top, right, bottom, left;

            if (Build.VERSION.SDK_INT >= 30) {
                android.graphics.Insets bars = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout() | WindowInsets.Type.ime());
                top = bars.top;
                right = bars.right;
                bottom = bars.bottom;
                left = bars.left;
            } else {
                // Includes the keyboard because of adjustResize in the manifest.
                top = insets.getSystemWindowInsetTop();
                right = insets.getSystemWindowInsetRight();
                bottom = insets.getSystemWindowInsetBottom();
                left = insets.getSystemWindowInsetLeft();
            }

            nativeInsets(top / density, right / density, bottom / density, left / density);
            return view.onApplyWindowInsets(insets);
        });
    }

    private void reportNightMode(Configuration config) {
        nativeNightMode((config.uiMode & Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES);
    }

    /** Starts a new input connection once iced has updated its text, because iced changed it without the keyboard's knowledge (Enter, cut, paste). */
    private void restartInputSoon() {
        imeView.postDelayed(() -> {
            if (imeView.hasFocus()) {
                getSystemService(InputMethodManager.class).restartInput(imeView);
            }
        }, 150);
    }

    // Called from Rust.

    /** Dark icons on a light app, light icons on a dark one. */
    @SuppressWarnings("deprecation")
    void setSystemBarsDark(boolean dark) {
        runOnUiThread(() -> {
            if (Build.VERSION.SDK_INT >= 30) {
                WindowInsetsController controller = getWindow().getInsetsController();
                int light = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS | WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;

                if (controller != null) {
                    controller.setSystemBarsAppearance(dark ? 0 : light, light);
                }
            } else {
                View decor = getWindow().getDecorView();
                int light = View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
                int flags = decor.getSystemUiVisibility() & ~light;
                decor.setSystemUiVisibility(dark ? flags : flags | light);
            }
        });
    }

    /** @param purpose 0 = normal text, 1 = password, 2 = terminal (no suggestions) */
    void showKeyboard(int purpose) {
        runOnUiThread(() -> {
            InputMethodManager manager = getSystemService(InputMethodManager.class);
            imeView.purpose = purpose;
            imeView.setFocusable(true);
            imeView.setFocusableInTouchMode(true);
            imeView.requestFocus();
            // Rust calls this on focus, purpose change and every tap into a focused field; each starts a fresh input connection (see ImeConnection).
            manager.restartInput(imeView);
            // Right after requestFocus the input method has not switched to the view yet and ignores showSoftInput, so ask once the focus change is through.
            imeView.post(() -> manager.showSoftInput(imeView, 0));
        });
    }

    void hideKeyboard() {
        runOnUiThread(() -> {
            if (Build.VERSION.SDK_INT >= 30) {
                getWindow().getInsetsController().hide(WindowInsets.Type.ime());
            } else {
                getSystemService(InputMethodManager.class).hideSoftInputFromWindow(imeView.getWindowToken(), 0);
            }

            imeView.clearFocus();
            imeView.setFocusable(false);
        });
    }

    /** Android's floating text toolbar at a point in the window, in dp. */
    void showTextMenu(float x, float y, boolean hasSelection) {
        runOnUiThread(() -> {
            if (textMenu != null) {
                textMenu.finish();
            }

            float density = getResources().getDisplayMetrics().density;
            int px = Math.round(x * density);
            int py = Math.round(y * density);

            textMenu = imeView.startActionMode(new ActionMode.Callback2() {
                @Override
                public boolean onCreateActionMode(ActionMode mode, Menu menu) {
                    if (hasSelection) {
                        menu.add(Menu.NONE, android.R.id.cut, 0, android.R.string.cut);
                        menu.add(Menu.NONE, android.R.id.copy, 1, android.R.string.copy);
                    }

                    if (getSystemService(ClipboardManager.class).hasPrimaryClip()) {
                        menu.add(Menu.NONE, android.R.id.paste, 2, android.R.string.paste);
                    }

                    menu.add(Menu.NONE, android.R.id.selectAll, 3, android.R.string.selectAll);
                    return true;
                }

                @Override
                public boolean onPrepareActionMode(ActionMode mode, Menu menu) {
                    return false;
                }

                @Override
                public boolean onActionItemClicked(ActionMode mode, MenuItem item) {
                    boolean handled = shortcut(item.getItemId());
                    mode.finish();
                    return handled;
                }

                @Override
                public void onDestroyActionMode(ActionMode mode) {
                    if (textMenu == mode) {
                        textMenu = null;
                    }
                }

                @Override
                public void onGetContentRect(ActionMode mode, View view, Rect outRect) {
                    // Relative to imeView, which sits at the window's top left corner.
                    outRect.set(px, py - 1, px + 1, py);
                }
            }, ActionMode.TYPE_FLOATING);
        });
    }

    void hideTextMenu() {
        runOnUiThread(() -> {
            if (textMenu != null) {
                textMenu.finish();
            }
        });
    }

    /** Sends a text menu or keyboard clipboard action to iced as its shortcut. */
    boolean shortcut(int id) {
        int action;

        if (id == android.R.id.cut) {
            action = 0;
        } else if (id == android.R.id.copy) {
            action = 1;
        } else if (id == android.R.id.paste) {
            action = 2;
        } else if (id == android.R.id.selectAll) {
            action = 3;
        } else {
            return false;
        }

        nativeShortcut(action);

        if (action != 1) {
            restartInputSoon();
        }

        return true;
    }

    String readClipboard() {
        ClipboardManager clipboard = getSystemService(ClipboardManager.class);
        ClipData data = clipboard.getPrimaryClip();

        if (data == null || data.getItemCount() == 0) {
            return null;
        }

        return data.getItemAt(0).coerceToText(this).toString();
    }

    void writeClipboard(String text) {
        ClipboardManager clipboard = getSystemService(ClipboardManager.class);
        clipboard.setPrimaryClip(ClipData.newPlainText("text", text));
    }

    void moveToBackground() {
        runOnUiThread(() -> moveTaskToBack(true));
    }

    /** An invisible view that holds keyboard focus, because NativeActivity's own view offers no input connection. */
    final class ImeView extends View {
        int purpose;

        ImeView(Context context) {
            super(context);
            // Focusable only while an iced field wants the keyboard (showKeyboard); otherwise Android 8 focuses the view at start and opens the keyboard.
            setFocusable(false);
        }

        @Override
        public boolean onCheckIsTextEditor() {
            return true;
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo info) {
            switch (purpose) {
                case 1:
                    info.inputType = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD;
                    break;
                case 2:
                    info.inputType = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS | InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD;
                    break;
                default:
                    info.inputType = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_AUTO_CORRECT;
            }

            // No fullscreen editor in landscape: the app shows the text itself.
            info.imeOptions = EditorInfo.IME_ACTION_DONE | EditorInfo.IME_FLAG_NO_EXTRACT_UI | EditorInfo.IME_FLAG_NO_FULLSCREEN;

            String[] context = purpose == 1 ? null : nativeTextContext();
            String before = context == null ? "" : context[0];
            String selected = context == null ? "" : context[1];
            String after = context == null ? "" : context[2];
            info.initialSelStart = before.length();
            info.initialSelEnd = before.length() + selected.length();

            if (Build.VERSION.SDK_INT >= 30) {
                // Lets the keyboard read the context right away, without a round trip through the connection.
                info.setInitialSurroundingText(before + selected + after);
            }

            return new ImeConnection(this, before, selected, after, nativeDeletesGraphemes());
        }
    }

    /**
     * Mirrors what the keyboard does into iced as key presses and commits.
     *
     * The connection's buffer starts with iced's text around the cursor, so the keyboard's predictions, autocorrect and capitalisation see the words already there.
     * After each edit the buffer is compared with what iced has: the changed range around iced's cursor becomes Backspace and Delete presses plus a commit, and cursor or selection moves become arrow presses.
     * A word being composed goes into iced's text right away rather than into iced's preedit overlay, which does not wrap and covers the text after the cursor.
     *
     * showKeyboard starts a new connection on every tap, because the tap may have moved iced's cursor; so do Enter and the text menu, after which iced's text is unknown here.
     */
    final class ImeConnection extends BaseInputConnection {
        private final View view;
        /** Whether iced's Backspace and Delete remove a grapheme (text_input) or a single code point (text_editor). */
        private final boolean deletesGraphemes;
        /** The text iced has, as far as the buffer covers it. */
        private String sent;
        /** iced's selection in `sent`; equal without a selection. */
        private int sentStart;
        private int sentEnd;
        private int batchDepth;

        ImeConnection(View view, String before, String selected, String after, boolean deletesGraphemes) {
            super(view, true);
            this.view = view;
            this.deletesGraphemes = deletesGraphemes;
            sent = before + selected + after;
            sentStart = before.length();
            sentEnd = before.length() + selected.length();
            Editable editable = getEditable();
            editable.append(sent);
            Selection.setSelection(editable, sentStart, sentEnd);
        }

        @Override
        public boolean beginBatchEdit() {
            batchDepth++;
            return super.beginBatchEdit();
        }

        @Override
        public boolean endBatchEdit() {
            batchDepth = Math.max(0, batchDepth - 1);
            boolean result = super.endBatchEdit();
            sync();
            return result;
        }

        @Override
        public boolean commitText(CharSequence text, int newCursorPosition) {
            boolean result = super.commitText(text, newCursorPosition);
            sync();
            return result;
        }

        @Override
        public boolean setComposingText(CharSequence text, int newCursorPosition) {
            boolean result = super.setComposingText(text, newCursorPosition);
            sync();
            return result;
        }

        @Override
        public boolean finishComposingText() {
            boolean result = super.finishComposingText();
            sync();
            return result;
        }

        @Override
        public boolean deleteSurroundingText(int beforeLength, int afterLength) {
            boolean result = super.deleteSurroundingText(beforeLength, afterLength);
            sync();
            return result;
        }

        @Override
        public boolean deleteSurroundingTextInCodePoints(int beforeLength, int afterLength) {
            boolean result = super.deleteSurroundingTextInCodePoints(beforeLength, afterLength);
            sync();
            return result;
        }

        @Override
        public boolean setSelection(int start, int end) {
            boolean result = super.setSelection(start, end);
            sync();
            return result;
        }

        @Override
        public boolean performContextMenuAction(int id) {
            // Keyboards' own clipboard and select-all buttons.
            return shortcut(id) || super.performContextMenuAction(id);
        }

        @Override
        public boolean sendKeyEvent(KeyEvent event) {
            int code = event.getKeyCode();

            // Backspace and Enter must go through the buffer, otherwise buffer and iced drift apart. Other keys go to NativeActivity and winit.
            if (code == KeyEvent.KEYCODE_ENTER || code == KeyEvent.KEYCODE_NUMPAD_ENTER) {
                if (event.getAction() == KeyEvent.ACTION_DOWN) {
                    performEditorAction(EditorInfo.IME_ACTION_DONE);
                }

                return true;
            }

            Editable editable = getEditable();
            int start = Selection.getSelectionStart(editable);
            int end = Selection.getSelectionEnd(editable);

            if (code == KeyEvent.KEYCODE_DEL && end > 0) {
                if (event.getAction() == KeyEvent.ACTION_DOWN) {
                    editable.delete(start < end ? start : Character.offsetByCodePoints(editable, end, -1), end);
                    sync();
                }

                return true;
            }

            return super.sendKeyEvent(event);
        }

        @Override
        public boolean performEditorAction(int action) {
            // The word being composed belongs before the line break or submit. Keyboards often wrap this in a batch edit, so send the pending edits now instead of at its end.
            super.finishComposingText();
            flush();
            nativeImeKey(2);
            // iced now has a line break, or the app may have changed the field on submit. Until the new connection has iced's new context, continue on an empty buffer.
            Editable editable = getEditable();
            removeComposingSpans(editable);
            editable.clear();
            sent = "";
            sentStart = 0;
            sentEnd = 0;
            reportSelection();
            restartInputSoon();
            return true;
        }

        /** Sends the buffer's changes to iced, unless the keyboard is in the middle of a batch edit. */
        private void sync() {
            if (batchDepth == 0) {
                flush();
            }
        }

        private void flush() {
            Editable editable = getEditable();
            String text = editable.toString();
            int start = Math.max(0, Selection.getSelectionStart(editable));
            int end = Math.max(0, Selection.getSelectionEnd(editable));
            int selectionStart = Math.min(start, end);
            int selectionEnd = Math.max(start, end);

            if (!text.equals(sent)) {
                edit(text);
            }

            moveSelection(selectionStart, selectionEnd);
            reportSelection();
        }

        /** Turns the difference between `sent` and `text` into presses and a commit at iced's cursor. */
        private void edit(String text) {
            // The changed range is sent[prefix, sent.length() - suffix). It must contain iced's selection or cursor, because iced only edits there.
            int prefix = 0;
            int prefixLimit = Math.min(sentStart, text.length());

            while (prefix < prefixLimit && sent.charAt(prefix) == text.charAt(prefix)) {
                prefix++;
            }

            int suffix = 0;
            int suffixLimit = Math.min(sent.length() - sentEnd, text.length() - prefix);

            while (suffix < suffixLimit && sent.charAt(sent.length() - 1 - suffix) == text.charAt(text.length() - 1 - suffix)) {
                suffix++;
            }

            // Count in whole graphemes (👍🏽 is one), which iced never splits.
            BreakIterator graphemes = graphemes(sent);
            prefix = graphemes.isBoundary(prefix) ? prefix : graphemes.preceding(prefix);
            int changedEnd = sent.length() - suffix;
            changedEnd = graphemes.isBoundary(changedEnd) ? changedEnd : graphemes.following(changedEnd);
            suffix = sent.length() - changedEnd;

            String inserted = text.substring(prefix, text.length() - suffix);

            if (sentStart < sentEnd) {
                // The first Backspace removes iced's selection.
                nativeImeKey(0);
            }

            repeat(0, deletions(prefix, sentStart));
            repeat(1, deletions(sentEnd, changedEnd));

            if (!inserted.isEmpty()) {
                nativeImeCommit(inserted);
            }

            sent = text;
            sentStart = prefix + inserted.length();
            sentEnd = sentStart;
        }

        /** Moves iced's cursor or selection to the buffer's with arrow presses, which move by graphemes in all iced text fields. */
        private void moveSelection(int start, int end) {
            if (start == sentStart && end == sentEnd) {
                return;
            }

            if (sentStart < sentEnd) {
                // Left collapses iced's selection to its start.
                nativeImeKey(3);
                sentEnd = sentStart;
            }

            BreakIterator graphemes = graphemes(sent);

            if (start < sentStart) {
                repeat(3, count(graphemes, start, sentStart));
            } else {
                repeat(4, count(graphemes, sentStart, start));
            }

            repeat(5, count(graphemes, start, end));
            sentStart = start;
            sentEnd = end;
        }

        /** How many Backspace or Delete presses remove sent[from, to). */
        private int deletions(int from, int to) {
            if (from >= to) {
                return 0;
            }

            return deletesGraphemes ? count(graphemes(sent), from, to) : sent.codePointCount(from, to);
        }

        private BreakIterator graphemes(String text) {
            BreakIterator iterator = BreakIterator.getCharacterInstance();
            iterator.setText(text);
            return iterator;
        }

        private int count(BreakIterator graphemes, int from, int to) {
            if (from >= to) {
                return 0;
            }

            int count = 0;

            for (int at = graphemes.following(from); at != BreakIterator.DONE && at <= to; at = graphemes.next()) {
                count++;
            }

            return count;
        }

        private void repeat(int key, int times) {
            for (int i = 0; i < times; i++) {
                nativeImeKey(key);
            }
        }

        /** Tells the keyboard where cursor and composing text are; editors must do this after every change, as TextView does. */
        private void reportSelection() {
            Editable editable = getEditable();
            view.getContext().getSystemService(InputMethodManager.class).updateSelection(view, Selection.getSelectionStart(editable), Selection.getSelectionEnd(editable), getComposingSpanStart(editable), getComposingSpanEnd(editable));
        }
    }
}
