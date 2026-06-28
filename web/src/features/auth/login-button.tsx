import { Button } from "@/shared/ui";

export function LoginButton() {
  return (
    <Button
      variant="primary"
      onClick={() => {
        window.location.href = "/v1/auth/github/login";
      }}
    >
      Sign in with GitHub
    </Button>
  );
}
