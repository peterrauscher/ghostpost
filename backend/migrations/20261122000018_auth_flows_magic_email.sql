-- Headless Magic Auth sign-in: bind the email address to the auth flow at
-- send-code time so verify-code can reject state/email mixups server-side.
-- Nullable: flows created by the hosted OAuth authorize path have no email.
ALTER TABLE auth_flows ADD COLUMN IF NOT EXISTS email text;
