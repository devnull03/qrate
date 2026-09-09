# Feedback email receipts

## Status

This document records a future requirement. The beta feedback flow does not send email receipts.

## Recommendation

Use Resend Transactional Email after Linear confirms issue creation.

Resend fits the current Cloudflare Worker design:

- The Worker can use one HTTPS request.
- The free tier allows 3,000 emails each month and 100 each day.
- Resend supports idempotency keys for 24 hours.
- An API key can have access to one sender domain.
- Resend publishes a data processing agreement and a subprocessor list.

Do not send a private Linear URL. Send only the public ticket identifier, such as `TSGB-123`.

Do not include the report text, diagnostics, logs, attachments, or user environment in the email.

Use plain text. Disable open tracking and click tracking.

## Proposed flow

1. Validate the optional email address.
2. Validate Turnstile and the feedback report.
3. Create the Linear issue.
4. Return feedback success even if the email later fails.
5. If the user supplied an email address, send one receipt.
6. Use the Linear issue ID and a recipient digest in the idempotency key.
7. Retry only temporary network and provider errors.

Use an idempotency key with this form:

```text
feedback-receipt/<linear-issue-id>/<recipient-digest>
```

The receipt can use this content:

```text
Subject: qrate received your feedback: TSGB-123

qrate received your feedback.

Ticket: TSGB-123

Keep this ticket number if you need to contact the qrate team.
```

Do not add a resend endpoint during the beta.

## Failure behavior

Email delivery is a secondary action. A receipt failure must not change a successful feedback result.

Use `waitUntil()` for a best-effort beta receipt. Do not describe this method as durable delivery.

If receipts become a contractual requirement, add a queue and explicit delivery state. Do not add this system before it is necessary.

Log only these values:

- Linear issue ID.
- Provider response status.
- Provider message ID.
- A one-way digest of the recipient address.

Do not log the email address or email body.

## Provider comparison

| Provider | Cost at beta volume | Idempotency | Main tradeoff |
|---|---:|---|---|
| Resend | 3,000 emails each month and 100 each day at no cost | 24-hour request key | Adds one processor with 30-day free-plan retention |
| Cloudflare Email Sending | 3,000 each month with Workers Paid | No documented request key | Fewer vendors, but the outbound service is in beta |
| Postmark | 100 each month at no cost | No provider key | Mature delivery, but higher cost and 45-day message retention |
| MailChannels | 100 each day at no cost, with account restrictions | No documented request key | Pricing and verified-recipient rules need confirmation |
| Amazon SES | Low unit cost | No provider key | Adds AWS, IAM, sandbox approval, and more operations |

Cloudflare Email Routing does not provide general outbound transactional email. Recheck Cloudflare Email Sending after it leaves beta.

## Privacy requirements

The email provider receives personal data:

- Recipient email address.
- Sender address.
- Ticket identifier.
- Delivery and bounce metadata.

Before release:

1. Add the provider and receipt purpose to the privacy notice.
2. Review the provider data processing agreement.
3. Review the provider subprocessor list and retention policy.
4. Use a dedicated sender subdomain.
5. Add SPF, DKIM, return-path, and DMARC records.
6. Disable marketing, open, and click tracking.
7. Configure bounce and suppression handling.

Do not use a user value for the subject, sender, reply-to address, header, or recipient list.

## Abuse controls

Turnstile does not prevent all email abuse. A user can submit a third party's email address.

Keep these controls:

- One receipt for each confirmed issue and recipient.
- Existing IP rate limits.
- A fresh Turnstile check before any report.
- No receipt attachments.
- No user-controlled email content.
- No public resend action.

## Launch checklist

- [ ] Create a dedicated receipt sender subdomain.
- [ ] Configure SPF, DKIM, return-path, and DMARC records.
- [ ] Add a restricted Resend API key as a Worker secret.
- [ ] Send only after Linear returns a confirmed issue.
- [ ] Use a deterministic idempotency key.
- [ ] Keep email failure non-fatal.
- [ ] Disable open and click tracking.
- [ ] Update the privacy notice.
- [ ] Test success, timeout, retry, suppression, bounce, and duplicate submission.
- [ ] Test a submission without an email address.

## Sources

- [Cloudflare Email Service](https://developers.cloudflare.com/email-service/)
- [Cloudflare Email Service pricing](https://developers.cloudflare.com/email-service/platform/pricing/)
- [Cloudflare outbound email setup](https://developers.cloudflare.com/email-service/get-started/send-emails/)
- [Resend pricing](https://resend.com/pricing)
- [Resend domain setup](https://resend.com/docs/add-a-domain)
- [Resend API keys](https://resend.com/docs/dashboard/api-keys/introduction)
- [Resend idempotency keys](https://resend.com/docs/dashboard/emails/idempotency-keys)
- [Resend data processing agreement](https://resend.com/legal/dpa)
- [Postmark pricing](https://postmarkapp.com/pricing)
- [Postmark email API](https://postmarkapp.com/developer/api/email-api)
- [Postmark idempotency guidance](https://postmarkapp.com/support/article/what-is-an-idempotency-key)
- [MailChannels Email API pricing](https://docs.mailchannels.com/email-api/billing/pricing)
- [MailChannels authentication](https://docs.mailchannels.com/email-api/authentication)
- [Amazon SES pricing](https://aws.amazon.com/ses/pricing/)
- [Amazon SES sandbox requirements](https://aws.amazon.com/ses/faqs/)
