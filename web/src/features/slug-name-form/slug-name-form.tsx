import { useState } from "react";
import { Modal, Field, Button, ErrorMessage } from "@/shared/ui";
import { useMutation } from "@/shared/lib";

interface Props {
  title: string;
  open: boolean;
  onClose: () => void;
  onCreate: (input: { slug: string; name: string }) => Promise<unknown>;
  onCreated: () => void;
}

export function SlugNameForm({
  title,
  open,
  onClose,
  onCreate,
  onCreated,
}: Props) {
  const [slug, setSlug] = useState("");
  const [name, setName] = useState("");

  const create = useMutation(onCreate, () => {
    setSlug("");
    setName("");
    onCreated();
    onClose();
  });

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
            loading={create.loading}
            disabled={!slug || !name}
            onClick={() => create.run({ slug, name })}
          >
            Create
          </Button>
        </>
      }
    >
      <Field
        label="Slug"
        value={slug}
        placeholder="my-app"
        onChange={(e) => setSlug(e.target.value)}
      />
      <Field
        label="Name"
        value={name}
        placeholder="My App"
        onChange={(e) => setName(e.target.value)}
      />
      {create.error && <ErrorMessage message={create.error.message} />}
    </Modal>
  );
}
