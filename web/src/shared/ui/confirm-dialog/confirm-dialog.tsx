import { Modal } from "@/shared/ui/modal/modal";
import { Button } from "@/shared/ui/button/button";
import { ErrorMessage } from "@/shared/ui/error-message/error-message";
import type { ReactNode } from "react";

interface Props {
  title: string;
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
  loading?: boolean;
  error?: string;
  confirmLabel?: string;
  children?: ReactNode;
}

export function ConfirmDialog({
  title,
  open,
  onClose,
  onConfirm,
  loading,
  error,
  confirmLabel = "Delete",
  children,
}: Props) {
  return (
    <Modal
      title={title}
      open={open}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="danger" loading={loading} onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children}
      {error && <ErrorMessage message={error} />}
    </Modal>
  );
}
