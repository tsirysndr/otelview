import { http, HttpResponse } from "msw";
import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { server } from "../../test/server";
import { renderApp } from "../../test/utils";
import { AuthGate } from "./AuthGate";

/** The gate asks the server two things: "how do I sign in here?" and "can
 * I read anything?". These drive both answers. */
function serverSays(
  info: Record<string, unknown>,
  statsStatus: number,
  statsBody = "unauthorized",
) {
  server.use(
    http.get("/auth/info", () => HttpResponse.json(info)),
    http.get("/api/stats", () =>
      statsStatus === 200
        ? HttpResponse.json({
            spans: 0,
            logs: 0,
            metric_points: 0,
            services: 0,
            backend: "memory",
          })
        : new HttpResponse(statsBody, { status: statsStatus }),
    ),
  );
}

const workspace = <p>the workspace</p>;

describe("AuthGate", () => {
  it("shows the workspace when the instance needs no credentials", async () => {
    serverSays({ mode: "none", static_token_accepted: false }, 200);
    renderApp(<AuthGate>{workspace}</AuthGate>);
    expect(await screen.findByText("the workspace")).toBeInTheDocument();
  });

  it("offers single sign-on when the server is behind a provider", async () => {
    serverSays(
      {
        mode: "oidc",
        login_url: "/auth/login",
        issuer: "https://auth.example.com",
        static_token_accepted: false,
      },
      401,
    );
    renderApp(<AuthGate>{workspace}</AuthGate>);

    const button = await screen.findByTestId("sso-sign-in");
    // A real navigation to the server's login endpoint, carrying where to
    // come back to — not a fetch, because the provider needs to show its
    // own pages for passwords, MFA and passkeys.
    expect(button).toHaveAttribute("href", expect.stringContaining("/auth/login"));
    expect(button).toHaveAttribute("href", expect.stringContaining("return_to="));
    expect(screen.getByText(/auth\.example\.com/)).toBeInTheDocument();
    expect(screen.queryByText("the workspace")).not.toBeInTheDocument();
  });

  it("prompts for a token when that is what the instance uses", async () => {
    serverSays({ mode: "token", static_token_accepted: true }, 401);
    renderApp(<AuthGate>{workspace}</AuthGate>);

    expect(await screen.findByLabelText("Access token")).toBeInTheDocument();
    expect(screen.queryByTestId("sso-sign-in")).not.toBeInTheDocument();
  });

  /** Signed in and still refused: offering the login again would just
   * loop them through the provider and back to the same wall. */
  it("explains a missing role instead of offering to sign in again", async () => {
    serverSays(
      { mode: "oidc", login_url: "/auth/login", static_token_accepted: false },
      403,
      "this account has none of the roles this instance requires",
    );
    renderApp(<AuthGate>{workspace}</AuthGate>);

    expect(await screen.findByText(/does not have access/i)).toBeInTheDocument();
    expect(screen.getByText(/none of the roles/)).toBeInTheDocument();
    expect(screen.queryByTestId("sso-sign-in")).not.toBeInTheDocument();
    expect(screen.getByText("sign out")).toBeInTheDocument();
  });

  it("falls back to the token prompt if the mode cannot be read", async () => {
    server.use(
      http.get("/auth/info", () => new HttpResponse("nope", { status: 500 })),
      http.get("/api/stats", () => new HttpResponse("unauthorized", { status: 401 })),
    );
    renderApp(<AuthGate>{workspace}</AuthGate>);
    await waitFor(() =>
      expect(screen.getByLabelText("Access token")).toBeInTheDocument(),
    );
  });
});
