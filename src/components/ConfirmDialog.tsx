import { useEffect, useRef } from "react";

interface ConfirmDialogProps {
  /** Rendered only while true; the dialog is modal for as long as it is. */
  open: boolean;
  title: string;
  /** What the reader is agreeing to. Name the thing being acted on here. */
  body: React.ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * A modal confirmation for an action that cannot be undone.
 *
 * Built on the native `<dialog>` rather than a div: focus trapping,
 * Escape-to-dismiss, an inert backdrop and top-layer painting all come from the
 * platform. The grid this is used over scrolls and its cards carry their own
 * buttons, so a hand-rolled overlay would have to re-implement every one of
 * those and lose to a stacking context somewhere.
 */
export function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const ref = useRef<HTMLDialogElement | null>(null);
  // Escape closes a native dialog through the DOM without telling React, which
  // would leave `open` true against a closed dialog -- and `showModal()` on an
  // already-open dialog throws, so the *next* delete would break rather than
  // this one. The `cancel` event is where that gets synced back.
  const onCancelRef = useRef(onCancel);
  onCancelRef.current = onCancel;

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) {
      return;
    }
    if (open && !dialog.open) {
      dialog.showModal();
    } else if (!open && dialog.open) {
      dialog.close();
    }
  }, [open]);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) {
      return;
    }
    function handleCancel(event: Event) {
      // Let React drive the close, so state and DOM cannot disagree.
      event.preventDefault();
      onCancelRef.current();
    }
    dialog.addEventListener("cancel", handleCancel);
    return () => dialog.removeEventListener("cancel", handleCancel);
  }, []);

  return (
    <dialog
      aria-labelledby="confirm-dialog-title"
      className="max-w-md rounded-md border border-neutral-200 bg-white p-0 text-neutral-900 backdrop:bg-neutral-950/50 dark:border-neutral-800 dark:bg-neutral-900 dark:text-neutral-100"
      ref={ref}
    >
      <div className="p-5">
        <h2 className="text-base font-semibold" id="confirm-dialog-title">
          {title}
        </h2>
        <div className="mt-2 text-sm text-neutral-600 dark:text-neutral-300">
          {body}
        </div>
        <div className="mt-5 flex justify-end gap-2">
          {/*
            Cancel first in the DOM so it takes the dialog's initial focus.
            `showModal` focuses the first focusable child, and on a destructive
            confirmation that should never be the destructive button -- Enter
            straight after opening would then delete the book.
          */}
          <button
            className="inline-flex h-9 items-center rounded-md border border-neutral-200 px-4 text-sm font-medium hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-800 dark:hover:bg-neutral-800"
            onClick={onCancel}
            type="button"
          >
            Cancel
          </button>
          <button
            className="inline-flex h-9 items-center rounded-md bg-red-700 px-4 text-sm font-medium text-white hover:bg-red-600 focus:outline-none focus:ring-2 focus:ring-red-500"
            onClick={onConfirm}
            type="button"
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </dialog>
  );
}
