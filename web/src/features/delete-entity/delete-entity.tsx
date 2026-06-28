import { useState } from "react";
import { ConfirmDialog, Checkbox } from "@/shared/ui";
import { useMutation } from "@/shared/lib";
import type { DeleteOptions } from "@/shared/api";

interface Props {
  title: string;
  entityName: string;
  open: boolean;
  withDataOption?: boolean;
  forceOption?: boolean;
  onClose: () => void;
  onDelete: (opts: DeleteOptions) => Promise<unknown>;
  onDeleted: () => void;
}

export function DeleteEntityDialog({
  title,
  entityName,
  open,
  withDataOption = false,
  forceOption = false,
  onClose,
  onDelete,
  onDeleted,
}: Props) {
  const [force, setForce] = useState(false);
  const [withData, setWithData] = useState(false);

  const del = useMutation(
    () => onDelete({ force, with_data: withData }),
    () => {
      onDeleted();
      onClose();
    },
  );

  return (
    <ConfirmDialog
      title={title}
      open={open}
      onClose={onClose}
      loading={del.loading}
      error={del.error?.message}
      onConfirm={() => del.run(undefined)}
    >
      <p>
        Delete <strong>{entityName}</strong>? This cannot be undone.
      </p>
      {forceOption && (
        <Checkbox
          label="Force (delete even if not empty)"
          checked={force}
          onChange={(e) => setForce(e.target.checked)}
        />
      )}
      {withDataOption && (
        <Checkbox
          label="Also remove docker volumes and backups"
          checked={withData}
          onChange={(e) => setWithData(e.target.checked)}
        />
      )}
    </ConfirmDialog>
  );
}
