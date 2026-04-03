-- Add new enum values to provider type
ALTER TYPE "public"."provider" ADD VALUE IF NOT EXISTS 'threads';

ALTER TYPE "public"."provider" ADD VALUE IF NOT EXISTS 'linkedin';
