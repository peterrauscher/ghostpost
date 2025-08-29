import { Lucia, TimeSpan } from "lucia";
import { DrizzlePostgreSQLAdapter } from "@lucia-auth/adapter-drizzle";
import type { Context } from "hono";
import { createDb } from "../db/client";
import { sessions, users } from "../db/schema";

type Env = { DATABASE_URL: string };

export function createLucia(c: Context<{ Bindings: Env }>) {
  const db = createDb(c.env.DATABASE_URL);
  const adapter = new DrizzlePostgreSQLAdapter(db, sessions, users);
  const lucia = new Lucia(adapter, {
    sessionExpiresIn: new TimeSpan(30, "d"),
    sessionCookie: {
      attributes: {
        secure: true,
        httpOnly: true,
        sameSite: "lax",
      },
    },
    getUserAttributes: (data) => ({
      email: data.email,
    }),
  });
  return lucia;
}

declare module "lucia" {
  interface Register {
    Lucia: ReturnType<typeof createLucia>;
    DatabaseUserAttributes: {
      email: string | null;
    };
  }
}
