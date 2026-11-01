# MailCraft VIP

MailCraft (branche `vip-merge`) avec les fonctions de VIP Mail, l'ancien client web d'Artisan Digital. Licence inchangée : MIT ou Apache-2.0, au choix.

## Ce que VIP Mail apporte

Chaque fonction est une commande. On la retrouve dans le ruban, dans la recherche de commandes (« Search commands », en haut à droite), dans la CLI et dans le serveur MCP.

| Fonction | Commandes | Dans l'interface |
|---|---|---|
| Expéditeurs VIP : catégorie « VIP » et importance haute, sur le courrier existant et à l'arrivée | `vip.add`, `vip.remove`, `vip.list` ; recherche `category:VIP` | Accueil › Indicateurs › VIP (étoile) |
| Reporter un message : il revient non lu, en haut de la liste | `mail.snooze`, `mail.unsnooze` ; recherche `is:snoozed` | Accueil › Indicateurs › Snooze (horloge) |
| Relance si pas de réponse | `compose.followUp`, `followup.list` | Message › Indicateurs › Follow Up |
| Envoi différé | `compose.delay` | Message › Indicateurs › Delay |
| Modèles de message | `template.save`, `template.remove`, `template.list`, `insert.quickParts` | Message › Inclure › Templates |
| Réponse automatique (absence), pour tout serveur IMAP | `account.automaticReplies` | Fichier › Infos › Automatic Replies |
| Désabonnement d'une liste (RFC 2369 / 8058) | `mail.unsubscribe`, `ui.unsubscribe` | Accueil › Indicateurs › Unsubscribe |
| Agenda Nextcloud (CalDAV), en lecture, à chaque Envoyer/Recevoir | `account.nextcloud` | Fichier › Infos › Nextcloud Calendar |

Ces fonctions se déclenchent sur le courrier qui arrive dans la boîte de réception (`crates/engine/src/cmd/vip.rs`, `arrived`) :
- marquage VIP ;
- fin de la relance quand une réponse arrive ;
- réponse automatique.

La réponse automatique part une seule fois par expéditeur. Elle ne part jamais vers les listes de diffusion, les mails en masse, les messages automatiques (RFC 3834) ni les adresses `noreply`. Elle ne répond pas non plus au courrier arrivé avant son activation.

### Fonctions non portées

| Fonction de VIP Mail | Statut |
|---|---|
| Règles de tri | Les règles de MailCraft (`rules.*`) sont plus complètes. |
| Libellés | Remplacés par les catégories de MailCraft. |
| Recherche Meilisearch | Remplacée par la recherche locale de MailCraft : hors ligne, sur toutes les boîtes, avec opérateurs. L'index de VIP Mail ne contenait que 79 mails, jamais nettoyés. |
| Tri par IA, résumé, réponses suggérées | Pas dans l'app. Claude Code passe par le serveur MCP `mailcraft` (`mailcraft-cli mcp --connect 127.0.0.1:7991`, déjà enregistré). |
| CalDAV (Nextcloud) | Porté, **en lecture** : l'agenda « Nextcloud » est en lecture seule ici. Pour créer ou modifier, passer par Nextcloud. Les contacts Nextcloud (CardDAV) ne sont **pas** synchronisés : MailCraft VIP sert à suivre la branche intégrée sur le parc, pas à synchroniser le carnet d'adresses. |
| Actions Dolibarr / Bench depuis un mail (devis, tiers, tâche) | Compétence Claude Code `mailcraft-dolibarr` (`~/Nextcloud/Claude/skills/`) : elle lit le mail par le MCP et appelle l'API. Le rattachement d'un mail à un projet n'a jamais servi et n'est pas repris. |
| Multi-utilisateur, PWA, Electron, APK | Hors sujet pour une app de bureau. |

## Installation sur un poste vierge

### Linux (CachyOS / Arch)

1. Installer Rust (le mot de passe sudo est demandé) :
   ```sh
   sudo pacman -S --needed rust git
   ```
2. Cloner le dépôt sur la branche `vip-merge` :
   ```sh
   git clone -b vip-merge <dépôt> ~/Code/mailcraft-vip
   ```
3. Installer (le lanceur démarre l'app avec `--control 7991`, pour le MCP) :
   ```sh
   ~/Code/mailcraft-vip/packaging/vip-install.sh
   ```
   Le script installe :
   - les binaires `mailcraft` et `mailcraft-cli` dans `~/.local/bin` ;
   - les icônes PNG `mailcraft-vip` dans le thème hicolor ;
   - le lanceur `io.github.billsecond.mailcraft.desktop`.
4. Épingler au dock ii (Quickshell) : ajouter `io.github.billsecond.mailcraft` à `dock.pinnedApps` dans `~/.config/illogical-impulse/config.json`. Faire un backup avant.

### macOS

1. Installer Rust :
   ```sh
   brew install rustup && rustup-init -y
   ```
2. Compiler l'app :
   ```sh
   cargo build --release -p mailcraft -p mailcraft-cli
   ```
3. Lancer `target/release/mailcraft`. Le paquet `.dmg` se construit avec `packaging/macos/`.

## Reprendre les données de VIP Mail

```sh
python3 -I ~/Nextcloud/Claude/migration/vipmail/import-vipmail.py
```

- Fermer MailCraft avant de lancer l'import.
- Le script lit `vipmail.sql` (dump Postgres de Bernard) et n'ajoute que ce qui manque. On peut donc le relancer sans risque.
- Il importe :
  - le compte `contact@artisan-digital.eu` (klorel.o2switch.net, IMAP 993 TLS, SMTP 465 TLS) ;
  - le carnet d'adresses ;
  - les modèles ;
  - la signature (texte seul) ;
  - le texte de la réponse automatique.
- Les expéditeurs VIP se listent dans `vip-senders.txt`, à côté du script.
- Il n'importe pas le mot de passe. Au premier Envoyer/Recevoir, MailCraft le demande une fois et le garde dans le trousseau du système. Le mot de passe se trouve dans `~/Nextcloud/Claude/password/credentials.md`.
- `--dry-run` affiche ce qui serait importé, sans rien écrire.

## Nextcloud

Dans Fichier › Infos › Nextcloud Calendar, renseigner :
- l'adresse : `https://cloud.happyinformatique.com` ;
- l'identifiant : `arnaud` ;
- un mot de passe d'application Nextcloud (Paramètres › Sécurité).

Le chemin par défaut est celui de Nextcloud (agenda `personal`). Les contacts déjà descendus par une version précédente restent : délier puis relier Nextcloud (adresse vide, OK, puis la remettre) les efface. Le mot de passe va dans le trousseau. L'agenda va d'un mois en arrière à un an en avant ; les séries sont dépliées par le serveur.

## Compte CEVA (Microsoft 365)

MailCraft se connecte à Microsoft 365 en OAuth. Le tenant CEVA bloque les nouveaux consentements : Microsoft affiche « Approbation requise ». MailCraft dit alors quoi faire et propose un court message à copier pour un administrateur.

**Ne pas relancer de connexion sur le tenant CEVA sans accord.**

Le jeton `ms-cockpit-token.json` ne sert pas de solution de repli :
- il n'autorise que `Mail.Read` : pas d'envoi, pas d'IMAP, pas d'agenda ;
- il appartient à une autre application Microsoft, et MailCraft n'utilise jamais l'identifiant d'une autre application.

Deux pistes restent possibles :
1. demander à l'IT CEVA d'approuver l'application MailCraft, avec le message proposé par l'app ;
2. continuer avec Outlook pour CEVA.

## Compte Gmail (connexion Google)

Avec un identifiant client Google, un compte Gmail se connecte sur la page de Google (OAuth 2.0,
PKCE, jeton XOAUTH2 vers `imap.gmail.com` / `smtp.gmail.com`) : plus de mot de passe
d'application. Sans identifiant, Gmail reste en mot de passe d'application comme avant.

1. Google Cloud Console → un projet → API et services → **Écran de consentement OAuth** : type
   « Externe », ajouter le scope `https://mail.google.com/`, puis **Publier l'application**
   (statut « En production »). En statut « Test », Google invalide le jeton tous les 7 jours.
   Pas besoin de validation Google pour un usage personnel : un avertissement « application non
   validée » s'affiche à la première connexion (Paramètres avancés → Continuer).
2. **Identifiants** → Créer → ID client OAuth → type **Application de bureau**.
3. Mettre l'ID et le secret dans `.env` (ignoré par git) et les exporter à la compilation **ou**
   au lancement :

   ```zsh
   export MAILCRAFT_GOOGLE_CLIENT_ID="xxxx.apps.googleusercontent.com"
   export MAILCRAFT_GOOGLE_CLIENT_SECRET="GOCSPX-…"
   ```

   Google documente le secret d'une application de bureau comme non confidentiel ; il n'est
   jamais écrit dans la boîte ni dans les journaux.
4. Fichier › Ajouter un compte → adresse `@gmail.com` → la page Google s'ouvre dans le
   navigateur. Le jeton de renouvellement va dans le trousseau du système.

### Libellés Gmail = catégories

Sur un compte Gmail (`imap.gmail.com`, extension `X-GM-EXT-1`), les libellés sont lus et écrits
comme des **catégories** MailCraft, dans les deux sens :

- à la réception, les libellés de l'utilisateur deviennent les catégories du message (les
  libellés système `\Inbox`, `\Important`, `\Starred`… restent des dossiers/drapeaux) ; un
  libellé inconnu est ajouté à la liste des catégories avec sa propre couleur ;
- catégoriser / décatégoriser un message dans MailCraft pose / retire le libellé dans Gmail à la
  synchro suivante (Gmail crée le libellé s'il n'existe pas) ;
- fusion à trois voies comme pour les drapeaux : un changement fait ici et pas encore poussé
  gagne, le reste suit le serveur.

À savoir : les catégories posées localement (règles, catégorie VIP) partent aussi dans Gmail
comme libellés. Les dossiers-libellés restent synchronisés comme avant (un message avec le
libellé « Clients » apparaît aussi dans le dossier Clients).

## Arrêt de VIP Mail sur Bernard

Fait le 19/10/2026 (`docker compose stop`, données conservées). Pour relancer au besoin :

```sh
ssh arnaud@bernard 'cd ~/Code/vipmail && docker compose start'
```

Commande d'arrêt :

```sh
ssh arnaud@bernard 'cd ~/Code/vipmail && docker compose stop'
```

Les données restent dans les volumes Docker et dans les dumps de `~/Nextcloud/Claude/migration/vipmail/`.

## Développement

- Tests VIP :
  ```sh
  cargo test -p mailcraft-engine vip
  cargo test -p mailcraft-model vip
  ```
- CI complète : `cargo xtask ci`. L'étape wasm a besoin de la cible `wasm32-unknown-unknown`, qui demande rustup ; le Rust de pacman ne la fournit pas.
