# MailCraft VIP Web — webmail privé type OWA (cadrage)

> Décidé le 30/10/2026 par Arnaud : un webmail **maison**, privé, construit **sur le moteur MailCraft VIP**
> (et non sur la version navigateur officielle). Branche de travail : `vip-ucs` → `vip-web`.

## Pourquoi pas la version navigateur officielle

La build WebAssembly de MailCraft (`apps/mailcraft-web`) ne peut pas ouvrir de connexion IMAP/SMTP :
elle ne parle qu'à des serveurs JMAP, **en lecture seule** pour l'instant (pas d'envoi, rien ne remonte).
Pour `contact@artisan-digital.eu` (IMAP/SMTP o2switch), il faudrait migrer la boîte vers un serveur JMAP
(Stalwart). On garde cette voie en veille et on suit l'upstream (#226, #233).

## Principe

Le moteur MailCraft tourne **côté serveur** (ikoula), sans fenêtre, une boîte par utilisateur. Une interface
web façon Outlook Web App le pilote par **les mêmes commandes** que le bureau, le CLI et MCP (registre de
commandes, `engine.execute`). On obtient la parité fonctionnelle au fil de l'eau, sans réécrire le métier.

```
navigateur (SPA charte Artisan Digital)
   │  HTTPS + SSO (webmail.artisan-digital.eu, Caddy)
   ▼
mailcraft-webd (nouveau binaire Rust, apps/mailcraft-webd)
   ├─ Session moteur par utilisateur  (/srv/mailcraft/<user>/mailbox.mailcraft)
   ├─ Send/Receive périodique         (même chemin que `mailcraft-cli sync`, crate net)
   ├─ Sauvegarde en arrière-plan      (format 3, journal)
   └─ Rendu HTML assaini côté serveur (crate html de MailCraft)
   ▼
IMAP/SMTP o2switch (contact@), Nextcloud CalDAV/CardDAV, Gmail (OAuth) plus tard
```

## API (première version)

| Route | Rôle |
|---|---|
| `POST /api/exec` `{command, params}` | toute commande du registre ; classes agent `safe`/`owner` autorisées, `never` refusées |
| `GET /api/folders`, `GET /api/list?folder&query&page` | arbre des dossiers, liste paginée (syntaxe de recherche MailCraft) |
| `GET /api/message/:id` | message complet, corps HTML **assaini** + texte |
| `GET /api/attachment/:id/:n` | pièce jointe (téléchargement, jamais rendue inline) |
| `GET /api/events` (SSE) | nouveaux messages, progression Send/Receive |

## Interface (OWA-like)

Barre Courrier / Calendrier / Contacts · volet dossiers · liste (conversations, filtres Non lus / Avec PJ /
VIP) · volet de lecture · rédaction **HTML** (barre de mise en forme, signatures Brand Kit par personne,
réponses/transferts, PJ, envoi différé, relance si pas de réponse, modèles « Saved Text ») · catégories,
VIP, snooze, désabonnement · recherche · responsive (téléphone) puis PWA.

## Sécurité

- Accès d'abord **Tailscale uniquement**, puis public derrière SSO si voulu (cookie `ad_sso` de la suite
  Artisan Digital ou Google OIDC + liste blanche, comme VIP Mail).
- Mot de passe IMAP/SMTP de contact@ : coffre Vaultwarden → fichier 0600 / variable au démarrage ; jamais
  dans la boîte ni dans git.
- Corps des mails : assainis côté serveur, affichés dans une `iframe sandbox` sans script, CSP stricte,
  images distantes bloquées par défaut (comme le bureau) ; CSRF et limitation de débit sur l'API.
- Sauvegarde restic de `/srv/mailcraft`.

## Jalons

1. **M1 Lire** : `mailcraft-webd`, SSO, dossiers / liste / lecture / recherche, Send/Receive de contact@.
2. **M2 Écrire** : rédaction HTML, signatures Brand Kit (Arnaud, Thanh), réponses, transferts, PJ, envoi.
3. **M3 VIP** : catégories, VIP, snooze, relances, modèles, réponses automatiques.
4. **M4** : calendrier et contacts (Nextcloud), mobile/PWA.
5. **M5** : comptes Gmail (OAuth) et multi-comptes.

## Points ouverts

- Domaine : `webmail.artisan-digital.eu` ou `mail.artisan-digital.eu`.
- Deux boîtes serveur (Arnaud, Thanh) sur le même compte contact@ : IMAP reste la source de vérité.
- Garder `apps/mailcraft-webd` et le front dans des dossiers à part pour limiter les conflits aux rebases
  sur `UCS-Care-Group/main`.
