---
title: "Design History"
description: Why Vision stays separate from implementation detail.
sidebar:
  order: 5
---

Design History records why user-facing philosophy lives apart from low-level
authority semantics. It explains past exploration without asking anyone to
restore those runtimes.

## From one North Star page to Vision plus authority

The earlier [Architecture North Star](../architecture/principles/north-star.md)
combined the product promise with Catalog Head, ETag compare-and-swap,
publication chains, Iceberg objects, Change descriptors, and other authority
semantics on one page. That combination made it hard to answer whether a
statement was a user promise or an implementation invariant.

Vision pages now carry the user-facing philosophy: what operators own, how
Knowledge, Work, and Experience differ, and where the product is going. The
Architecture North Star remains the implementation authority for catalog roots,
publication order, Form history ownership, and adapter boundaries.

## Canvas and code-sandbox experiments

Earlier Canvas and code-sandbox experiments helped expose the durable
requirement: Knowledge should remain user-owned while different tools can be
built around it. They are design history, not a request to restore those
runtimes. A successful render was never evidence of durable persistence, and
generated tools must not copy Knowledge into an opaque application database.

## What this separation protects

New contributors read [The Ugoite Vision](index.md) and
[Product Principles](principles.md) before Architecture implementation docs.
Operators check [Current Product and Target State](current-and-target.md) before
assuming a North Star capability ships. Implementation detail stays in
Architecture and Specification, where validators and tests can check it.
