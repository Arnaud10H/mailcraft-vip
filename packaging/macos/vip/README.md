# MailCraft VIP sur Mac — installation et mises à jour simples

Pour un poste non technique (Thanh) : deux raccourcis, rien d'autre à connaître.

| Raccourci | Ce qu'il fait |
|---|---|
| **MailCraft VIP** (Dock, Bureau) | ouvre le client mail |
| **Mettre à jour MailCraft** (Dock, Bureau) | récupère la dernière version de notre Forgejo, la fabrique, sauvegarde la boîte, remplace l'appli et la relance ; si c'est déjà à jour, propose « Réinstaller quand même » |

## Première installation (une fois, par Arnaud ou son Claude)

```zsh
curl -fsSL -H "Authorization: token <JETON>" \
  "https://git.artisan-digital.eu/arnaud/mailcraft-vip/raw/branch/vip-ucs/packaging/macos/vip/vip-mac-setup.sh" \
  | bash -s -- <JETON>
```

`<JETON>` : jeton Forgejo **en lecture seule** (Forgejo › Paramètres › Applications, portée `read:repository`),
gardé ensuite dans le trousseau du Mac. Le script installe les outils Apple et Rust, clone `vip-ucs`
dans `~/Applications/MailCraft-VIP/src`, crée les raccourcis et fait la première fabrication.

## Détails

- L'appli s'installe dans `~/Applications/MailCraft VIP.app` (pas besoin de droits administrateur).
- Fabriquée avec `MAILCRAFT_BUILD_SHA=dev` : elle ne propose **jamais** les mises à jour d'UCS (qui n'ont
  pas nos fonctions VIP) ; les mises à jour passent uniquement par le raccourci.
- Boîte : `~/Library/Application Support/MailCraft`, sauvegardée avant chaque mise à jour
  (3 dernières dans `MailCraft-sauvegardes`).
- Journal : `~/Library/Logs/MailCraft-VIP-mise-a-jour.log`.
