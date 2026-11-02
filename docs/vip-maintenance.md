# MailCraft VIP — maintenance (référence pour Claude et pour Arnaud)

> À lire en premier dans toute session MailCraft. Mis à jour le 31/10/2026.

## Où sont les choses

| Quoi | Où |
|---|---|
| Dépôt de travail | JeanClaude `~/Code/mailcraft-vip` (toujours sur `vip-ucs`, propre) |
| Worktree de synchro | JeanClaude `~/Code/mailcraft-vip-sync` (branche `vip-sync`, jetable) |
| Forgejo (base commune) | `ssh://git@ikoula:2223/arnaud/mailcraft-vip.git` — web https://git.artisan-digital.eu/arnaud/mailcraft-vip |
| Upstream actif | `upstream` = https://github.com/UCS-Care-Group/mailcraft (≈100 commits/jour) |
| Ancien upstream | `origin` = billsecond/mailcraft — **figé depuis le 15/10**, ne plus s'en servir |
| App installée | `~/.local/bin/mailcraft` (+ `-cli`), lanceur `io.github.billsecond.mailcraft.desktop` (`--control 7991`) |
| Boîte | `~/.config/mailcraft/mailbox.mailcraft` (compte contact@artisan-digital.eu) |
| Sauvegardes boîte | `~/Backups/mailcraft/mailbox-*.tgz` (5 dernières) |
| Journaux de synchro | `~/.local/state/mailcraft-vip-sync/*.log`, rapports `~/Nextcloud/Claude/system-reports/mailcraft-sync-*.md` |
| Cadrage webmail | `docs/vip-webmail.md` · fonctions VIP pour l'utilisateur : `README-VIP.md` |

Accès depuis une session Cowork : la VM de l'appareil « jeanclaude » joint l'hôte en SSH,
`ssh -i <copie 0600 de ~/Nextcloud/Claude/password/ssh-cowork/id_cowork_sandbox> arnaud@10.0.2.2`.
Une commande `device_bash` dure 180 s au plus : lancer les compilations et les tests avec
`systemd-run --user --collect --unit=<nom> -p LimitNOFILE=65536 …` et relire leur journal.

## Branches

- **`vip-ucs`** : la branche de référence (rebase du 30/10 sur UCS 0.8.2 + VIP1–VIP9). Elle n'est
  **jamais réécrite** : on y fusionne l'upstream (merge), update-arch et le Mac la tirent en fast-forward.
- `vip-sync` : branche de travail de la synchro (worktree). `vip-sync-AAAAMMJJ` : tentative en échec
  poussée pour examen. `vip-merge` : ancienne branche (base billsecond), gelée.

## Synchroniser avec l'upstream

`packaging/vip-sync-upstream.sh` (une tâche planifiée le lance chaque nuit) :

1. `--dry-run` : ce qui est nouveau et les fichiers qui seraient en conflit.
2. sans option : merge `upstream/main` dans `vip-sync`. Les `.po`/`.pot` en conflit prennent la
   version upstream (ils sont régénérés). Code 2 = conflits à résoudre dans le worktree.
3. Après résolution et commit dans le worktree : `--continue`.
4. Contrôles : `cargo xtask i18n`, `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`
   (avec `ulimit -n 65536`, sinon des centaines de tests réseau échouent en « Too many open files »).
5. Tout vert : `vip-sync` est poussée comme `vip-ucs`, le dépôt principal avance, la boîte est
   sauvegardée et l'app réinstallée (`packaging/vip-install.sh`). Code 3 = contrôle en échec,
   `vip-ucs` intacte.

## Résoudre un conflit : les points d'accroche VIP

Règle : **garder ce que fait l'upstream et y ajouter le comportement VIP** ; ne jamais supprimer de la
logique upstream pour faire passer un patch VIP. Si l'upstream livre l'équivalent d'une fonction VIP
(connexion Google #232, modèles, envoi différé — listés comme manquants dans leur ROADMAP), le signaler
à Arnaud et proposer de retirer le doublon VIP au profit de l'upstream ; ne pas trancher seul.

| Fichier | Accroche VIP |
|---|---|
| `engine/src/cmd/account.rs` | Outbox : `m.send_at` (envoi différé) ; nouveau mail de l'Inbox : `super::vip::arrived` juste avant `apply_rules` (dans `if inbox != sorted`) ; `kind: "dav"` / `dav_server` / `dav_shown` (compte CalDAV, refus de `sync.request`) ; Gmail : `label_changes` / `merge_labels` / `register_labels` / `labels_pushed`, `RemoteOp::Labels` dans la boucle des changements (note en `Some(…)`) ; imports `LabelsPushed`, `own_labels` |
| `engine/src/cmd/compose.rs` | `send_at: c.send_at` dans `build` ; `vip::mark_follow_up` dans `send` ; `vip::arrived` à la livraison locale |
| `engine/src/cmd/mod.rs` | `pub mod vip;`, `vip::specs()`, `mod dav_account_tests` |
| `engine/src/list.rs` | filtre snooze (`snoozed_until`, `query.wants_snoozed()`), arrivée = fin du snooze |
| `engine/src/session.rs` | `Compose.send_at`, `Compose.follow_up_days` |
| `engine/src/agent.rs` | classes des commandes VIP (SAFE / SEND / ACCOUNTS / SETTINGS) — toute nouvelle commande VIP doit y être classée (test `every_command_has_a_class`) |
| `model/src/message.rs` | `snoozed_until`, `send_at`, `awaiting_reply`, `synced_labels` |
| `model/src/mailbox.rs` | `Mailbox.vip`, `AccountKind::Dav` (+ `is_dav`), `Account.dav: Option<DavServer>` ; `model/src/dav_server.rs` |
| `model/src/search.rs` | `Kind::Snoozed`, `is:snoozed` (pas dans `IS_VALUES`, sinon tests de suggestion), `wants_snoozed` |
| `model/src/sync.rs` | `FolderDelta.labels`, `ServerLabels`, `own_labels`, `LabelsPushed`, `SyncResult.labeled`, `pushed_count` |
| `model/src/provider.rs` | preset `google()` |
| `net/src/` | `dav/` (mod, read, events, write, standin, tests : job CalDAV/CardDAV, `DavJob` → `DavOutcome`) ; `http.rs` (`Reply.location` / `Reply.etag`) ; `lib.rs` (`test_account` → `dav::check`) ; `sync.rs` (libellés) ; `push.rs` (`RemoteOp::Labels`) ; `fake.rs` (`FakeMessage.labels`) ; `oauth.rs` (dialecte Google) |
| `proto/src/imap/mod.rs` | `gmail_labels`, `items()`, `X-GM-LABELS` dans `uid_fetch_flags` (avec `Wanted`/`response_cap` de l'upstream) |
| `ui-egui/src/` | `forms.rs` (`AutoReply` dans tous les `match`) ; `backstage.rs` (tuiles VIP, `account_detail` pour `Dav`) ; `ribbon.rs` (Snooze, VIP, Unsubscribe, Templates, Delay…) ; `lib.rs` (`Services::dav`, `DavRequest`, `start_dav`, `dav_accounts`, `forget_password`, `secret_key` pour `Dav`, `can_reach`) ; `calendar_sync.rs` (`dav_job`, comptes Dav dans `calendar_accounts`, mot de passe refusé) ; `account_setup.rs` (connexion Google ; `Kind::Dav`, page serveur, `dav_value`) |
| `apps/mailcraft/src/main.rs` | `google_client_id()` ; `dav` / `dav_in` (job CalDAV) ; `mod dav_flow` (test de bout en bout) |

Pièges vus au rebase du 30/10 : libellés interdits par `product_names_tests` (pas « Outlook »,
« Office », « Quick Parts »…) ; textes du ruban absents de `messages.pot` → `cargo xtask i18n` ;
nouveaux champs de structs upstream à compléter dans les initialisations de test.

## Comptes et secrets

- contact@artisan-digital.eu (IMAP/SMTP o2switch) ; mot de passe dans le trousseau du système.
- Signatures HTML Brand Kit posées le 30/10 : « Arnaud — Artisan Digital » (défaut) et
  « Thanh — Artisan Digital » (script `~/.cache/mc-sig/apply_sig.py`, source https://branding.artisan-digital.eu).
- Connexion Google (Gmail) : `MAILCRAFT_GOOGLE_CLIENT_ID/SECRET` dans `.env` du dépôt ou
  `~/Nextcloud/Claude/password/mailcraft-google.env` (lu à la compilation par update-arch). Jamais dans git.

## Autres postes

- Edouard (Mac) : brief `~/Nextcloud/Claude/migration/BRIEF-Edouard-MailCraft-vip-ucs-20261030.md`.
- **Tout poste Linux (JeanClaude, Bernard…)** : `packaging/vip-update.sh` — clone s'il le faut (`~/Code/mailcraft-vip`),
  fast-forward de `vip-ucs` depuis Forgejo, compile avec l'identifiant Google s'il est déposé, sauvegarde la boîte,
  installe (`vip-install.sh`), relance l'app ; `--check` dit seulement s'il y a du nouveau, `--no-restart` pour
  update-arch/cron, `--force` pour réinstaller. Poste vierge :
  `bash <(curl -fsSL https://git.artisan-digital.eu/arnaud/mailcraft-vip/raw/branch/vip-ucs/packaging/vip-update.sh)`.
- update-arch (JeanClaude) : appelle `~/Code/mailcraft-vip/packaging/vip-update.sh --no-restart` **avant** les paquets système.

## Automatisation

- **Minuteur systemd (JeanClaude)** : `packaging/systemd/mailcraft-vip-sync.{service,timer}` lancent
  `packaging/mc-sync-run.sh` chaque nuit à 04:47 : synchro, rapport `system-reports/mailcraft-sync-AAAAMMJJ.md`,
  notification. Les jours sans conflit tout est automatique ; un conflit (code 2) ou un contrôle en échec
  (code 3) attend la discussion MailCraft. Activation (une fois, par Arnaud) :
  `cp ~/Code/mailcraft-vip/packaging/systemd/mailcraft-vip-sync.* ~/.config/systemd/user/ && systemctl --user daemon-reload && systemctl --user enable --now mailcraft-vip-sync.timer`
- **Tâche planifiée Cowork (optionnelle)** : la même synchro menée par Claude, qui résout aussi les
  conflits. Prompt prêt : `~/Nextcloud/Claude/projets/mailcraft/TACHE-PLANIFIEE-synchro.md` (elle a besoin du
  dossier `Claude/password` pour la clé SSH : à créer et approuver par Arnaud).
