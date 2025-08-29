import { Hono } from "hono";
import { createLucia } from "./lucia";
import { createProviders } from "./providers";
import { createDb } from "../db/client";
import { userIdentities, users } from "../db/schema";
import { and, eq } from "drizzle-orm";

type Env = {
  DATABASE_URL: string;
  FACEBOOK_CLIENT_ID: string;
  FACEBOOK_CLIENT_SECRET: string;
  FACEBOOK_REDIRECT_URI: string;
  TWITTER_CLIENT_ID: string;
  TWITTER_CLIENT_SECRET: string;
  TWITTER_REDIRECT_URI: string;
};

export const auth = new Hono<{ Bindings: Env }>();

auth.get("/login/facebook", async (c) => {
  const providers = createProviders({
    facebookClientId: c.env.FACEBOOK_CLIENT_ID,
    facebookClientSecret: c.env.FACEBOOK_CLIENT_SECRET,
    facebookRedirectUri: c.env.FACEBOOK_REDIRECT_URI,
    twitterClientId: c.env.TWITTER_CLIENT_ID,
    twitterClientSecret: c.env.TWITTER_CLIENT_SECRET,
    twitterRedirectUri: c.env.TWITTER_REDIRECT_URI,
  });
  const state = crypto.randomUUID();
  c.header(
    "Set-Cookie",
    `oauth_state=${state}; Path=/; HttpOnly; SameSite=Lax; Secure`
  );
  const url = providers.facebook.createAuthorizationURL({
    state,
    scope: ["email", "public_profile"],
  });
  return c.redirect(url.toString());
});

auth.get("/callback/facebook", async (c) => {
  const lucia = createLucia(c);
  const db = createDb(c.env.DATABASE_URL);
  const providers = createProviders({
    facebookClientId: c.env.FACEBOOK_CLIENT_ID,
    facebookClientSecret: c.env.FACEBOOK_CLIENT_SECRET,
    facebookRedirectUri: c.env.FACEBOOK_REDIRECT_URI,
    twitterClientId: c.env.TWITTER_CLIENT_ID,
    twitterClientSecret: c.env.TWITTER_CLIENT_SECRET,
    twitterRedirectUri: c.env.TWITTER_REDIRECT_URI,
  });

  const url = new URL(c.req.url);
  const code = url.searchParams.get("code");
  const state = url.searchParams.get("state");
  const cookieState = c.req.header("Cookie")?.match(/oauth_state=([^;]+)/)?.[1];
  if (!code || !state || !cookieState || state !== cookieState)
    return c.text("Invalid state", 400);

  const tokens = await providers.facebook.validateAuthorizationCode(code);
  const userInfo = await providers.facebook.userinfo(tokens.accessToken);

  // find or create user
  const provider = "facebook" as const;
  const providerUserId = userInfo.id;
  const email = (userInfo.email ?? null) as string | null;

  // ensure identity exists
  const existingIdentity = await db
    .select()
    .from(userIdentities)
    .where(
      and(
        eq(userIdentities.provider, provider),
        eq(userIdentities.providerUserId, providerUserId)
      )
    );

  let userId: string;
  if (existingIdentity.length) {
    userId = existingIdentity[0].userId as string;
  } else {
    // try to link by email
    let user = email
      ? (await db.select().from(users).where(eq(users.email, email))).at(0)
      : undefined;
    if (!user) {
      user = (
        await db
          .insert(users)
          .values({ id: crypto.randomUUID(), email })
          .returning()
      ).at(0);
    }
    userId = user!.id as string;
    await db.insert(userIdentities).values({
      userId,
      provider,
      providerUserId,
      accessToken: tokens.accessToken,
    });
  }

  // create session
  const session = await lucia.createSession(userId, {});
  const cookie = lucia.createSessionCookie(session.id);
  c.header("Set-Cookie", cookie.serialize(), { append: true });

  return c.redirect("/");
});

auth.get("/login/twitter", async (c) => {
  const providers = createProviders({
    facebookClientId: c.env.FACEBOOK_CLIENT_ID,
    facebookClientSecret: c.env.FACEBOOK_CLIENT_SECRET,
    facebookRedirectUri: c.env.FACEBOOK_REDIRECT_URI,
    twitterClientId: c.env.TWITTER_CLIENT_ID,
    twitterClientSecret: c.env.TWITTER_CLIENT_SECRET,
    twitterRedirectUri: c.env.TWITTER_REDIRECT_URI,
  });
  const state = crypto.randomUUID();
  c.header(
    "Set-Cookie",
    `oauth_state=${state}; Path=/; HttpOnly; SameSite=Lax; Secure`
  );
  const url = providers.twitter.createAuthorizationURL({
    state,
    scope: ["tweet.read", "users.read"],
  });
  return c.redirect(url.toString());
});

auth.get("/callback/twitter", async (c) => {
  const lucia = createLucia(c);
  const db = createDb(c.env.DATABASE_URL);
  const providers = createProviders({
    facebookClientId: c.env.FACEBOOK_CLIENT_ID,
    facebookClientSecret: c.env.FACEBOOK_CLIENT_SECRET,
    facebookRedirectUri: c.env.FACEBOOK_REDIRECT_URI,
    twitterClientId: c.env.TWITTER_CLIENT_ID,
    twitterClientSecret: c.env.TWITTER_CLIENT_SECRET,
    twitterRedirectUri: c.env.TWITTER_REDIRECT_URI,
  });

  const url = new URL(c.req.url);
  const code = url.searchParams.get("code");
  const state = url.searchParams.get("state");
  const cookieState = c.req.header("Cookie")?.match(/oauth_state=([^;]+)/)?.[1];
  if (!code || !state || !cookieState || state !== cookieState)
    return c.text("Invalid state", 400);

  const tokens = await providers.twitter.validateAuthorizationCode(code);
  const userInfo = await providers.twitter.userinfo(tokens.accessToken);

  const provider = "twitter" as const;
  const providerUserId = userInfo.id;
  const email = (userInfo.email ?? null) as string | null;

  const existingIdentity = await db
    .select()
    .from(userIdentities)
    .where(
      and(
        eq(userIdentities.provider, provider),
        eq(userIdentities.providerUserId, providerUserId)
      )
    );

  let userId: string;
  if (existingIdentity.length) {
    userId = existingIdentity[0].userId as string;
  } else {
    let user = email
      ? (await db.select().from(users).where(eq(users.email, email))).at(0)
      : undefined;
    if (!user) {
      user = (
        await db
          .insert(users)
          .values({ id: crypto.randomUUID(), email })
          .returning()
      ).at(0);
    }
    userId = user!.id as string;
    await db.insert(userIdentities).values({
      userId,
      provider,
      providerUserId,
      accessToken: tokens.accessToken,
    });
  }

  const session = await lucia.createSession(userId, {});
  const cookie = lucia.createSessionCookie(session.id);
  c.header("Set-Cookie", cookie.serialize(), { append: true });
  return c.redirect("/");
});
