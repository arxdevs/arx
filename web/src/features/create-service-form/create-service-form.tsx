import { useState } from "react";
import { Modal, Field, Button, ErrorMessage } from "@/shared/ui";
import { useMutation } from "@/shared/lib";
import {
  serviceApi,
  type CreateServiceInput,
} from "@/entities/service";
import styles from "./create-service-form.module.css";

interface Props {
  ws: string;
  proj: string;
  open: boolean;
  onClose: () => void;
  onCreated: () => void;
}

const KINDS: CreateServiceInput["kind"][] = ["git", "image", "db"];
const TEMPLATES = ["postgres", "mysql", "mongodb", "redis"];

export function CreateServiceForm({
  ws,
  proj,
  open,
  onClose,
  onCreated,
}: Props) {
  const [slug, setSlug] = useState("");
  const [name, setName] = useState("");
  const [kind, setKind] = useState<CreateServiceInput["kind"]>("git");
  const [repo, setRepo] = useState("");
  const [branch, setBranch] = useState("main");
  const [image, setImage] = useState("");
  const [template, setTemplate] = useState("postgres");

  const reset = () => {
    setSlug("");
    setName("");
    setRepo("");
    setImage("");
  };

  const create = useMutation(
    () => {
      const input: CreateServiceInput = { slug, name, kind };
      if (kind === "git") {
        input.repo = repo;
        input.branch = branch;
      } else if (kind === "image") {
        input.image = image;
      } else {
        input.template = template;
      }
      return serviceApi.create(ws, proj, input);
    },
    () => {
      reset();
      onCreated();
      onClose();
    },
  );

  const valid =
    slug &&
    name &&
    ((kind === "git" && repo) ||
      (kind === "image" && image) ||
      kind === "db");

  return (
    <Modal
      title="New service"
      open={open}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            loading={create.loading}
            disabled={!valid}
            onClick={() => create.run(undefined)}
          >
            Create
          </Button>
        </>
      }
    >
      <Field
        label="Slug"
        value={slug}
        placeholder="api"
        onChange={(e) => setSlug(e.target.value)}
      />
      <Field
        label="Name"
        value={name}
        placeholder="API"
        onChange={(e) => setName(e.target.value)}
      />

      <div className={styles.kinds}>
        {KINDS.map((k) => (
          <button
            key={k}
            type="button"
            className={`${styles.kind} ${kind === k ? styles.active : ""}`}
            onClick={() => setKind(k)}
          >
            {k}
          </button>
        ))}
      </div>

      {kind === "git" && (
        <>
          <Field
            label="Repository"
            value={repo}
            placeholder="owner/repo"
            onChange={(e) => setRepo(e.target.value)}
          />
          <Field
            label="Branch"
            value={branch}
            onChange={(e) => setBranch(e.target.value)}
          />
        </>
      )}

      {kind === "image" && (
        <Field
          label="Image"
          value={image}
          placeholder="nginx:latest"
          onChange={(e) => setImage(e.target.value)}
        />
      )}

      {kind === "db" && (
        <label className={styles.field}>
          <span className={styles.label}>Template</span>
          <select
            className={styles.select}
            value={template}
            onChange={(e) => setTemplate(e.target.value)}
          >
            {TEMPLATES.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </label>
      )}

      {create.error && <ErrorMessage message={create.error.message} />}
    </Modal>
  );
}
