# Projection To Cloud

`domain-criome` should project provider-neutral state:

- domain-name-system records;
- redirect rules;
- enough domain identity to let `cloud` choose the configured provider account.

The projection must not include provider names. Provider selection is `cloud`
policy: the domain component describes what should be true, and the cloud
component decides which configured provider can make it true.
