# Ledger App Full-Stack Coding Task

Develop a basic money transfer and balance tracking app with a global feed (think Venmo).

## Product Functionality

**1. Login**

Users can log in with a username and password. No need to integrate with a third party — basic auth is fine.

**2. Send money**

Users can find other users by username and send them money. The system must properly account for everyone's balances.

**3. Global feed**

A live-updating list of all transactions across the system. New transactions should appear automatically in real time (i.e., the user does not have to refresh the page to see new transactions appear).

**4. README**

Instructions for how to run the app, your stack choices, and any context you want to share about your approach.

## Notes

This is a money app. We care a lot about getting the money math right under realistic conditions. Use your best judgment on what that entails and be ready to defend your choices.

In particular, think about:

- How balances are represented and derived
- What happens when multiple transfers happen at the same time
- What happens when a client retries a request after a timeout or network failure
- How you would audit or reconstruct account history if something looked wrong

We do not require a full production banking ledger, but we do expect you to make deliberate choices here and explain the tradeoffs.

## Things to Think About

Before coding, we encourage you to spend a little time researching common approaches to money movement systems, especially:

- Integer-based money representation
- Append-only ledgers
- Double-entry accounting
- Database transactions and isolation
- Idempotency keys for retry-safe APIs

You do not need to implement every production-grade pattern you find. We care more that your implementation has clear invariants, realistic failure behavior, and that you can explain where it is strong or limited.

## Testing Expectations

Please include automated tests for the money movement logic.

At minimum, include a test that attempts 100 concurrent transfers from the same funded account and proves that:

- No account balance goes negative
- The total amount of money in the system is conserved
- Only the transfers that can be funded succeed
- Failed transfers do not leave partial state behind

Also include at least one test for retry behavior. For example, if the same transfer request is submitted twice because the client did not know whether the first request succeeded, the sender should not be charged twice.

## Tech Stack

You decide. Be ready to discuss and defend the reasoning behind your choice and think about strengths and weaknesses.

## Other notes

- No need for an admin page, registration page, password recovery, etc. Hardcode some users or add a seeding script with starting balances.
- You can use any libraries and dependencies you like.
- You can use AI to generate code, but please make sure you understand every line and are proud of it. The goal is to write a vertical slice of production-level code. During our review we will go through and talk through specifics of how your code works, your database schema so you will struggle if you simply paste this prompt into Claude!

## Delivery

- Create a private GitHub repository and add `danniss10` as a collaborator.
- An oral presentation and discussion of the above, including motivations for your technology and design choices. The discussion may broaden into more general technical questions, and we may pair-program some live changes or feature additions.

**Expected time:** ~4 hours +/- an hour. If you are approaching 6 hours, wrap up and get your code into a runnable state, then stop. We can discuss what you would have done with more time.

If you finish in less than 4 hours, keep pushing. This is an opportunity to show us what you can do! More important than speed, though, is maintaining correctness and a high-quality codebase.

Good luck!
