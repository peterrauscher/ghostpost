-- Enable pgcrypto for gen_random_uuid()
CREATE EXTENSION IF NOT EXISTS pgcrypto;

--> statement-breakpoint
CREATE TYPE "public"."provider" AS ENUM ('instagram', 'tiktok', 'twitter', 'facebook');

--> statement-breakpoint
CREATE TABLE
	"connected_accounts" (
		"id" uuid PRIMARY KEY DEFAULT gen_random_uuid () NOT NULL,
		"user_id" uuid NOT NULL,
		"provider" "provider" NOT NULL,
		"status" text DEFAULT 'not_connected' NOT NULL,
		"connected_at" timestamp
		with
			time zone
	);

--> statement-breakpoint
CREATE TABLE
	"profiles" (
		"id" uuid PRIMARY KEY NOT NULL,
		"created_at" timestamp
		with
			time zone DEFAULT now (),
			"display_name" text,
			"avatar_url" text
	);

--> statement-breakpoint
CREATE TABLE
	"sessions" (
		"id" text PRIMARY KEY NOT NULL,
		"user_id" uuid NOT NULL,
		"expires_at" timestamp
		with
			time zone NOT NULL,
			"created_at" timestamp
		with
			time zone DEFAULT now ()
	);

--> statement-breakpoint
CREATE TABLE
	"uploads" (
		"id" uuid PRIMARY KEY DEFAULT gen_random_uuid () NOT NULL,
		"user_id" uuid NOT NULL,
		"provider" "provider" NOT NULL,
		"file_name" text,
		"byte_size" integer,
		"status" text DEFAULT 'pending' NOT NULL,
		"created_at" timestamp
		with
			time zone DEFAULT now ()
	);

--> statement-breakpoint
CREATE TABLE
	"user_identities" (
		"id" uuid PRIMARY KEY DEFAULT gen_random_uuid () NOT NULL,
		"user_id" uuid NOT NULL,
		"provider" "provider" NOT NULL,
		"provider_user_id" text NOT NULL,
		"access_token" text,
		"refresh_token" text,
		"expires_at" timestamp
		with
			time zone,
			"scopes" text,
			"created_at" timestamp
		with
			time zone DEFAULT now ()
	);

--> statement-breakpoint
CREATE TABLE
	"users" (
		"id" uuid PRIMARY KEY NOT NULL,
		"email" text,
		"created_at" timestamp
		with
			time zone DEFAULT now ()
	);

--> statement-breakpoint
CREATE UNIQUE INDEX "connected_accounts_user_provider_unique" ON "connected_accounts" USING btree ("user_id", "provider");

--> statement-breakpoint
CREATE UNIQUE INDEX "identities_provider_user_unique" ON "user_identities" USING btree ("provider", "provider_user_id");

--> statement-breakpoint
CREATE UNIQUE INDEX "users_email_unique" ON "users" USING btree ("email");
