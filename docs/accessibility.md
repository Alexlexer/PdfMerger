# Accessibility testing

PdfMerger is designed so its core editing workflow does not require drag and drop.
This checklist records the current keyboard behavior and the manual checks required for
`v0.3.0`.

## Keyboard workflow

1. Use **Tab** and **Shift+Tab** to move through the menu bar, document-group controls,
   page controls, dialogs, active jobs, and diagnostic actions.
2. Use **Enter** or **Space** to activate the focused control. In a modal, **Enter** runs the
   primary action and **Escape** cancels it.
3. Modal dialogs block the workspace, keep keyboard focus inside the dialog, set an appropriate
   initial focus target, and return focus to the control that opened them.
4. Select pages with each page's labeled **Page NN** toggle.
5. Transfer selected pages without dragging by activating **Move selection here** on the
   destination document group.
6. Reorder pages with **Earlier** and **Later**, and reorder groups with **Move up** and
   **Move down**.
7. Use the documented global shortcuts for selection, rotation, deletion, undo/redo,
   import, project open/save, and export.

## Manual release checks

- Complete import, selection, reorder, transfer, rotation, removal, export, and cancellation
  using only the keyboard.
- Confirm every interactive control exposes a meaningful text label and enabled/disabled state.
- Confirm focused, hovered, selected, and destructive controls remain distinguishable.
- Check the dark theme in the operating system's high-contrast mode.
- Check layout and focus visibility at 100%, 125%, 150%, and 200% display scaling.
- Verify dialog focus does not escape behind the active modal.
- Exercise the application with a platform screen reader and record unlabeled or out-of-order
  controls as defects.

Automated model tests cover the mutations behind these controls. Screen-reader output and
platform high-contrast behavior remain manual checks until an appropriate GUI harness is added.