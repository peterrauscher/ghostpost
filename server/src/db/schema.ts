import {
  integer,
  pgEnum,
  pgTable,
  text,
  timestamp,
  uuid,
  uniqueIndex,
} from "drizzle-orm/pg-core";

export const providerEnum = pgEnum("provider", [
  "instagram",
  "tiktok",
  "twitter",
  "facebook",
  "threads",
  "linkedin",
]);

export const profiles = pgTable("profiles", {
  id: uuid("id").primaryKey(),
  createdAt: timestamp("created_at", { withTimezone: true }).defaultNow(),
  displayName: text("display_name"),
  avatarUrl: text("avatar_url"),
});

export const connectedAccounts = pgTable(
  "connected_accounts",
  {
    id: uuid("id").defaultRandom().primaryKey(),
    userId: uuid("user_id").notNull(),
    provider: providerEnum("provider").notNull(),
    status: text("status").notNull().default("not_connected"),
    connectedAt: timestamp("connected_at", { withTimezone: true }),
  },
  (table) => {
    return {
      userProviderUnique: uniqueIndex(
        "connected_accounts_user_provider_unique"
      ).on(table.userId, table.provider),
    };
  }
);

export const uploads = pgTable("uploads", {
  id: uuid("id").defaultRandom().primaryKey(),
  userId: uuid("user_id").notNull(),
  provider: providerEnum("provider").notNull(),
  fileName: text("file_name"),
  byteSize: integer("byte_size"),
  status: text("status").notNull().default("pending"),
  createdAt: timestamp("created_at", { withTimezone: true }).defaultNow(),
});

// Auth tables for Lucia
export const users = pgTable(
  "users",
  {
    id: uuid("id").primaryKey(),
    email: text("email"),
    createdAt: timestamp("created_at", { withTimezone: true }).defaultNow(),
  },
  (table) => {
    return {
      emailUnique: uniqueIndex("users_email_unique").on(table.email),
    };
  }
);

export const sessions = pgTable("sessions", {
  id: text("id").primaryKey(),
  userId: uuid("user_id").notNull(),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  createdAt: timestamp("created_at", { withTimezone: true }).defaultNow(),
});

export const userIdentities = pgTable(
  "user_identities",
  {
    id: uuid("id").defaultRandom().primaryKey(),
    userId: uuid("user_id").notNull(),
    provider: providerEnum("provider").notNull(),
    providerUserId: text("provider_user_id").notNull(),
    accessToken: text("access_token"),
    refreshToken: text("refresh_token"),
    expiresAt: timestamp("expires_at", { withTimezone: true }),
    scopes: text("scopes"),
    createdAt: timestamp("created_at", { withTimezone: true }).defaultNow(),
  },
  (table) => {
    return {
      providerUserUnique: uniqueIndex("identities_provider_user_unique").on(
        table.provider,
        table.providerUserId
      ),
    };
  }
);
