export type PricingPlanId = 'single' | 'sevenDay' | 'thirtyDay';

export type PricingPlan = {
  id: PricingPlanId;
  name: string;
  priceLabel: string;
  price: number;
  badge?: string;
  summary: string;
  perks: string[];
};

/** Launch offerings from Ghostpost pricing strategy (Jul 2026). */
export const PRICING_PLANS: PricingPlan[] = [
  {
    id: 'single',
    name: 'Single Scan',
    priceLabel: '$9.99',
    price: 9.99,
    summary: 'One full scan and its results',
    perks: ['Full flag review for this scan', 'No rescans', 'One-time purchase'],
  },
  {
    id: 'sevenDay',
    name: '7-Day Clean Slate Pass',
    priceLabel: '$14.99',
    price: 14.99,
    badge: 'Best value',
    summary: 'Full access + up to 10 rescans for 7 days',
    perks: [
      'Every flagged post unlocked',
      'Up to 10 rescans',
      'Up to 3 platforms',
      'No auto-renewal',
    ],
  },
  {
    id: 'thirtyDay',
    name: '30-Day Clean Slate Pass',
    priceLabel: '$24.99',
    price: 24.99,
    summary: 'Extended cleanup window with rescans',
    perks: ['Full access for 30 days', 'Rescans included', 'One-time purchase'],
  },
];

export const DEFAULT_PLAN_ID: PricingPlanId = 'sevenDay';
