import { Hono } from "hono";
import { cors } from "hono/cors";
import { z } from "zod";
import { zValidator } from "@hono/zod-validator";
import { createDb } from "./db/client";
import { connectedAccounts, profiles, users } from "./db/schema";
import { eq } from "drizzle-orm";
import { auth } from "./auth/routes";

type Env = {
  DATABASE_URL: string;
};

const app = new Hono<{ Bindings: Env }>();

app.use("*", cors());

app.get("/health", (c) => c.json({ ok: true }));

app.route("/auth", auth);

// Upsert profile
const upsertProfile = z.object({
  id: z.string().uuid(),
  displayName: z.string().optional(),
});
app.post("/profiles", zValidator("json", upsertProfile), async (c) => {
  const db = createDb(c.env.DATABASE_URL);
  const body = c.req.valid("json");
  await db
    .insert(profiles)
    .values({
      id: body.id,
      displayName: body.displayName,
    })
    .onConflictDoUpdate({
      target: profiles.id,
      set: { displayName: body.displayName },
    });
  return c.json({ ok: true });
});

// Connected accounts
const accountUpsert = z.object({
  userId: z.string().uuid(),
  provider: z.enum([
    "instagram",
    "tiktok",
    "twitter",
    "facebook",
    "threads",
    "linkedin",
  ]),
  status: z.string().default("connected"),
});
app.post(
  "/connected-accounts",
  zValidator("json", accountUpsert),
  async (c) => {
    const db = createDb(c.env.DATABASE_URL);
    const body = c.req.valid("json");
    // Ensure user exists (nullable email)
    await db.insert(users).values({ id: body.userId }).onConflictDoNothing();

    await db
      .insert(connectedAccounts)
      .values({
        userId: body.userId,
        provider: body.provider,
        status: body.status,
      })
      .onConflictDoUpdate({
        target: [connectedAccounts.userId, connectedAccounts.provider],
        set: { status: body.status },
      });
    return c.json({ ok: true });
  }
);

// Batch connected accounts upsert
const batchUpsert = z.object({
  userId: z.string().uuid(),
  providers: z
    .array(
      z.enum(["instagram", "tiktok", "twitter", "facebook", "threads", "linkedin"]) // accept extra but filter
    )
    .min(1),
  status: z.string().default("connected"),
});

app.post("/connected-accounts/batch", zValidator("json", batchUpsert), async (c) => {
  const db = createDb(c.env.DATABASE_URL);
  const body = c.req.valid("json");

  // Ensure user exists
  await db.insert(users).values({ id: body.userId }).onConflictDoNothing();

  const providers = body.providers.filter((p) =>
    ["instagram", "tiktok", "twitter", "facebook"].includes(p)
  ) as Array<"instagram" | "tiktok" | "twitter" | "facebook">;

  await Promise.all(
    providers.map((provider) =>
      db
        .insert(connectedAccounts)
        .values({ userId: body.userId, provider, status: body.status })
        .onConflictDoUpdate({
          target: [connectedAccounts.userId, connectedAccounts.provider],
          set: { status: body.status },
        })
    )
  );

  return c.json({ ok: true, count: providers.length });
});

app.get("/connected-accounts/:userId", async (c) => {
  const db = createDb(c.env.DATABASE_URL);
  const userId = c.req.param("userId");
  const rows = await db
    .select()
    .from(connectedAccounts)
    .where(eq(connectedAccounts.userId, userId));
  return c.json(rows);
});

export default app;
