import { useState } from "react";
import { Modal, Field, Button, ErrorMessage } from "@/shared/ui";
import { useMutation } from "@/shared/lib";

interface Props {
  title: string;
  open: boolean;
  current: string;
  onClose: () => void;
  onRename: (name: string) => Promise<unknown>;
  onRenamed: () => void;
}

export function RenameDialog({
  title,
  open,
  current,
  onClose,
  onRename,
  onRenamed,
}: Props) {
  const [name, setName] = useState(current);

  const rename = useMutation(
    () => onRename(name),
    () => {
      onRenamed();
      onClose();
    },
  );

  return (
    <Modal
      title={title}
      open={open}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            loading={rename.loading}
            disabled={!name}
            onClick={() => rename.run(undefined)}
          >
            Rename
          </Button>
        </>
      }
    >
      <Field
        label="Name"
        value={name}
        onChange={(e) => setName(e.target.value)}
      />
      {rename.error && <ErrorMessage message={rename.error.message} />}
    </Modal>
  );
}
