# Pigeonnier — spec

Nov 5, 2026 · @Bérenger

## Vision

Pigeonnier est un compagnon de bureau qui répond à une seule question : **qu'est-ce qui m'attend, projet par projet ?** Il agrège trois flux qui vivent aujourd'hui dans deux services et des centaines de repos.

- **PR à traiter** (GitHub) : reviews demandées, mes PR ouvertes qui attendent une action.
- **Actions manuelles** (Todoist) : ce que Claude m'a demandé de faire à la main, et mes autres todos.
- **Roadmap** (GitHub Projects) : les items datés ou prioritaires du projet de l'org.

### Pourquoi les outils existants échouent

Planify et Super Productivity organisent par **service** ou par **repo**. L'unité ici est le **projet**, qui correspond à une org GitHub, son GitHub Project et une section Todoist. Super Productivity exige en plus un provider par repo, ingérable avec des centaines de repos.

### Principes

- **Le projet d'abord** : jamais de liste « GitHub » ou « Todoist » à l'écran, toujours des projets.
- **Requêtes, pas repos** : GitHub est interrogé par recherche au niveau org ; aucun repo n'est configuré.
- **Les services restent la source de vérité** : Pigeonnier ne stocke qu'un cache, il ne duplique rien dans Todoist ni dans GitHub.
- **Local-first et sans serveur** : tokens dans le trousseau du système, aucun backend tiers.
- **Discret** : une sidebar étroite et un indicateur dans la barre GNOME suffisent au quotidien.

### Non-objectifs (v1)

- Pas de client GitHub complet : relire une PR se fait dans le navigateur ou VS Code.
- Pas de remplaçant de Todoist : créer et organiser des tâches complexes reste dans Todoist.
- Pas de time-tracking, de pomodoro ni de calendrier.
- Pas de multi-utilisateur.

## Concept central : le projet

Un projet Pigeonnier = **un owner GitHub (org ou compte perso) + son GitHub Project + une section Todoist**. Ta convention actuelle (une org = un Project = une section) rend le mapping **déductible automatiquement** : le fichier de config ne sert qu'aux exceptions.

### Découverte automatique au premier lancement

1. Lister les orgs dont je suis membre, plus mon compte perso.
2. Pour chaque owner, lister ses Projects v2 ; s'il n'y en a qu'un, il est retenu, sinon le plus récemment modifié et l'app demande confirmation.
3. Lister les sections du projet Todoist « parent » choisi (par ex. *Dev*) et les apparier par nom normalisé (casse, tirets, accents ignorés).
4. Proposer le résultat dans un écran de validation : chaque ligne est un projet, cochable, avec couleur et nom modifiables.
5. Écrire le résultat dans le fichier de config ; les owners sans section Todoist restent des projets « GitHub seul ».

### Fichier de config

Emplacement : `~/.config/pigeonnier/projects.toml` (XDG). Éditable à la main, rechargé à chaud.

```toml
[defaults]
todoist_parent = "Dev"          # projet Todoist qui contient une section par projet
claude_label   = "claude"       # label posé par Claude sur les actions manuelles
refresh        = "5m"

[[project]]
id      = "helpers4"
name    = "helpers4"
color   = "blue"                # palette GNOME ou hex
github  = { owner = "helpers4", project = 1 }
todoist = { section = "helpers4" }

[[project]]
id      = "perso"
name    = "Perso"
color   = "orange"
github  = { owner = "@me", project = 3, repos = ["baxyz/folioscope"] }  # filtre optionnel
todoist = { section = "Perso", project = "Inbox" }                    # surcharge du parent

[[project]]
id      = "sunny-tech"
name    = "Sunny Tech"
color   = "#e5a50a"
github  = { owner = "sunnytech" }                 # pas de Project : roadmap vide
todoist = { section = "Sunny Tech" }
```

### Règles

- Un item n'appartient qu'à **un** projet, résolu par l'owner du repo (GitHub) ou la section (Todoist).
- Un item orphelin (repo d'un owner non mappé, tâche hors section) tombe dans un projet virtuel **« Autres »**, replié par défaut.
- L'ordre des projets à l'écran est celui du fichier, réordonnable par glisser-déposer (l'app réécrit le fichier).
- Un projet peut être **épinglé** (toujours en haut) ou **en sourdine** (masqué de la sidebar, plus de notifications).

## Sources de données

Tout tient en **4 appels réseau par rafraîchissement, quel que soit le nombre de repos** : 2 recherches GitHub globales pour les PR, 1 requête par GitHub Project pour la roadmap, 1 sync incrémentale Todoist. Le routage vers les projets se fait ensuite en local.

### PR à traiter (GitHub GraphQL, API Search)

Deux recherches globales, **sans filtre d'org** : l'owner de chaque PR décide de son projet.

| Flux | Requête de recherche | Ce que ça attrape |
| --- | --- | --- |
| Reviews | `is:pr is:open archived:false review-requested:@me` | PR où ma review est demandée (moi ou une de mes teams) |
| Mes PR | `is:pr is:open archived:false author:@me` | Mes PR ouvertes, actionnables ou non |
| Assignées (option) | `is:pr is:open archived:false assignee:@me -author:@me` | PR d'autres qu'on m'a assignées |

Champs lus par PR : `number`, `title`, `url`, `isDraft`, `updatedAt`, `createdAt`, `repository { nameWithOwner owner { login } }`, `reviewDecision`, `mergeable`, `statusCheckRollup { state }`, `labels`, `milestone { dueOn }`.

**L'état « action requise »** est dérivé localement, du plus au moins urgent :

1. Review demandée → *à reviewer*
2. Ma PR avec `CHANGES_REQUESTED` → *à corriger*
3. Ma PR avec CI `FAILURE` ou `ERROR` → *CI rouge*
4. Ma PR `CONFLICTING` → *conflits*
5. Ma PR `APPROVED` + CI verte + `MERGEABLE` → *à merger*
6. Ma PR en attente de review ou en draft → *en attente* (affichée en gris, pas de notif)

Les PR de bots (Dependabot, Renovate) sont regroupées sur une ligne repliable « 7 mises à jour de dépendances » par projet, pour ne pas noyer le reste.

### Roadmap (GitHub Projects v2, GraphQL)

Une requête par projet mappé, paginée par 100 items : `organization(login:) { projectV2(number:) { … } }` (ou `user(login:)` pour le compte perso).

- **Schéma des champs** lu une fois et mis en cache : champs `SingleSelect` (statut, priorité, avec leurs couleurs), `Date`, `Iteration`.
- **Items** : `isArchived`, `content` (Issue, PullRequest ou DraftIssue, avec titre, URL, état, assignees), `fieldValues` (single-select, date, itération).
- **Filtre** : items non archivés, contenu ouvert, statut hors « Done ». Option par projet : seulement les items qui me sont assignés.

Le sens des champs varie d'un Project à l'autre, donc il est déclaré dans la config, avec des valeurs par défaut qui collent au template GitHub :

```toml
[project.roadmap]
status_field   = "Status"
done_values    = ["Done"]
priority_field = "Priority"          # options P0..P2 du template GitHub
date_field     = "Target date"       # ou iteration_field = "Iteration" (fin d'itération = deadline)
only_assigned  = false
```

### Todoist (API v1, endpoint /sync)

L'API v1 réunit les anciennes API REST et Sync sous une seule base, et `/sync` renvoie uniquement les changements depuis le dernier jeton ([source](https://freeapihub.com/apis/todoist-api)). Un seul appel suffit donc après la sync initiale.

- **Appel** : `POST https://api.todoist.com/api/v1/sync`, `resource_types=["items","sections","projects","labels"]`, `sync_token` persisté.
- **Routage** : `item.section_id` → projet Pigeonnier via le mapping ; tâches du parent sans section → projet « Autres ».
- **Actions manuelles de Claude** : tâches portant le label `claude`. C'est une convention à fixer côté Claude, par une ligne dans tes instructions ou un `CLAUDE.md` (« toute action manuelle créée dans Todoist porte le label `claude` et va dans la section du projet »).
- **Champs lus** : `content`, `description`, `priority`, `due` (date ou datetime, récurrence), `deadline`, `labels`, `checked`.
- **Piège** : l'API code la priorité de 1 (normale) à 4 (urgente), à l'inverse de l'affichage P1…P4 dans l'app ([source](https://pkg.go.dev/github.com/smirnoffmg/go-todoist@v0.1.0)).

### Budget d'appels

| Source | Appels par rafraîchissement | Limite |
| --- | --- | --- |
| GitHub GraphQL | 2 recherches + 1 par Project (≈ 5 à 10 en tout) | 5 000 points par heure |
| Todoist | 1 | 1 000 requêtes par 15 min ([source](https://freeapihub.com/apis/todoist-api)) |

Un rafraîchissement toutes les 5 minutes consomme une faible fraction de ces quotas. Un bouton et le focus de la fenêtre déclenchent aussi un rafraîchissement immédiat.

## Modèle de données unifié

Chaque élément, quelle que soit sa source, devient un `Item` avec **une priorité P1 à P4, une échéance et un état**. C'est ce qui permet de trier une PR, une tâche et un item de roadmap dans la même liste.

```rust
struct Item {
    id: ItemId,              // "gh-pr:helpers4/devcontainer#42", "td:8812345", "gh-pv2:PVTI_…"
    project: ProjectId,
    kind: Kind,              // Review | MyPr | ClaudeAction | Todo | Roadmap
    title: String,
    context: String,         // "devcontainer#42", "section helpers4", "Sprint 12"
    url: Url,                // ouverture dans le navigateur ou l'app native
    priority: Priority,      // P1 (urgent) .. P4 (aucune)
    due: Option<Due>,        // date ou datetime, récurrence ignorée à l'affichage
    deadline: Option<NaiveDate>,
    state: State,            // ActionRequired(Reason) | Waiting | Planned
    updated_at: DateTime<Utc>,
    snoozed_until: Option<DateTime<Utc>>,  // local uniquement
}
```

### Normalisation des priorités

| Source | Règle | P1 | P2 | P3 | P4 |
| --- | --- | --- | --- | --- | --- |
| Todoist | champ `priority` (inversé dans l'API) | 4 | 3 | 2 | 1 |
| Roadmap | option du champ priorité du Project | P0 | P1 | P2 | vide |
| PR | règles locales | label `urgent` ou `priority:high` | review demandée, CI rouge, changements demandés | à merger, conflits | en attente, draft, bots |

Règle d'âge pour les reviews : une review demandée depuis plus de 2 jours ouvrés monte d'un cran (P2 → P1). Le seuil se règle dans la config.

### Échéances

- **Todoist** : `deadline` si présente, sinon `due`. Les deux s'affichent si elles diffèrent (« prévu lun. · limite ven. »).
- **Roadmap** : champ date du Project, sinon fin de l'itération courante.
- **PR** : `dueOn` du milestone s'il y en a un, sinon aucune échéance.

### Tri dans un projet

1. En retard
2. Priorité (P1 d'abord)
3. Échéance la plus proche
4. Dernière mise à jour

L'état `Waiting` (ma PR en attente de review, par exemple) passe toujours en fin de liste, en gris.

### Code couleur

| Élément visuel | Porte | Valeurs |
| --- | --- | --- |
| Pastille et liseré du projet | l'identité du projet | couleur choisie dans la config (palette GNOME) |
| Drapeau | la priorité | P1 rouge, P2 orange, P3 bleu, P4 absent (convention Todoist) |
| Texte de l'échéance | l'urgence temporelle | rouge si dépassée, orange si aujourd'hui, gras sous 3 jours, gris sinon |
| Icône | le type | review, PR, action Claude, todo, roadmap (icônes symboliques Adwaita) |
| Badge d'état PR | la raison | « CI rouge », « conflits », « à merger »… |

La couleur ne porte jamais seule le sens : chaque drapeau a aussi son libellé au survol et pour les lecteurs d'écran.

### Report (snooze)

Une PR ou un item de roadmap ne peut pas être « reporté » dans GitHub. Pigeonnier garde donc un report **local** (à demain, lundi prochain, date libre), stocké dans le cache SQLite. Pour une tâche Todoist, le report modifie la vraie date `due`.

## Authentification au démarrage

Le premier lancement est un assistant de 3 écrans : **connecter GitHub, connecter Todoist, valider les projets découverts**. Les lancements suivants sont silencieux ; un token expiré ou révoqué affiche un bandeau « Reconnecter » en haut de la liste, sans bloquer le reste.

### GitHub : device flow

1. L'app affiche un code à 8 caractères et un bouton « Ouvrir github.com/login/device ».
2. Tu colles le code dans le navigateur et tu autorises.
3. L'app sonde le endpoint de token jusqu'à l'autorisation, puis passe à l'écran suivant.

Aucun serveur de callback, aucun port local, ça marche aussi sur Android et derrière un SSH. Il faut une **OAuth App** déclarée sur ton compte, device flow activé.

- **Scopes** : `repo` (PR des repos privés), `read:org` (liste des orgs), `read:project` (Projects v2).
- **Raccourci** : si `gh` est installé et connecté, proposer « Utiliser le compte de GitHub CLI » (lecture de `gh auth token`). Zéro clic sur ligno et fajro.
- **Plan B** : coller un fine-grained PAT.
- **Point d'attention** : une org qui restreint les OAuth Apps tierces doit approuver l'app. Pas de souci pour tes propres orgs ; pour les autres, le raccourci `gh` ou un PAT contourne le problème.

### Todoist : OAuth avec redirection locale

1. L'app ouvre le navigateur sur la page d'autorisation Todoist, scope `data:read_write`.
2. Après accord, Todoist redirige vers `http://127.0.0.1:<port fixe>/callback`, où l'app écoute brièvement.
3. L'app échange le code contre un token.

- **Contrainte** : l'échange du code demande le client secret de l'app Todoist. Une app open source ne peut pas le cacher vraiment ; Planify, par exemple, vit avec. À vérifier en premier : le support de PKCE et d'une URL de redirection en loopback par Todoist.
- **Plan B** : coller le token API personnel (Réglages Todoist → Intégrations → Développeur). C'est le plus simple pour un usage strictement personnel.
- **Android** : redirection via un deep link `pigeonnier://callback` si Todoist l'accepte, sinon token collé.

### Stockage des secrets

Tokens dans le trousseau natif via la crate `keyring` : Secret Service (GNOME Keyring) sur Linux, Credential Manager sur Windows, Keystore sur Android. Jamais en clair dans la config ni dans le cache.

### Déconnexion

Un bouton par service dans les préférences : suppression du token du trousseau et purge du cache de ce service.

## Architecture

Recommandation retenue : **un cœur Rust unique et deux UI**. libadwaita native sur GNOME, Tauri partagé entre Windows 11 et Android. Sur Linux, un démon sert l'app, l'extension GNOME et Claude.

&#91;embedded content: architecture · 2 sources, 1 démon, 3 clients\]

Sur GNOME, le démon tourne en permanence et l'app n'est qu'un client D-Bus. Sur Windows et Android, le même cœur tourne dans le processus de l'app Tauri.

### Choix de la stack

| Cible | UI | Pourquoi |
| --- | --- | --- |
| GNOME (Ubuntu) | libadwaita native, Rust + Relm4 | Démarrage instantané, look et comportement natifs, sidebar = point de rupture `AdwBreakpoint` |
| Windows 11 x64 et ARM64 | Tauri 2 + Fluent UI Web Components, effet Mica | WebView2 (Chromium) rapide, même UI qu'Android |
| Android | Tauri 2, layout mobile, petit plugin Kotlin pour l'arrière-plan | Même UI que Windows |
| KDE, iOS, macOS | Sur demande uniquement | Rien dans l'architecture ne l'empêche ; pour KDE, un plasmoïde QML |

Tauri est écarté de Linux parce que WebKitGTK est sa webview la plus lente. Sur Windows et Android, il garde le duo Rust + TypeScript et une seule UI pour deux cibles. Le prix : chaque fonctionnalité visible s'écrit deux fois, d'où la règle ci-dessous.

### Règle : les view-models vivent dans le cœur

- Le cœur fournit des **listes déjà triées et groupées** par projet.
- Il fournit aussi les **libellés prêts à afficher** (« à reviewer depuis 3 j », « aujourd'hui »), les **couleurs sémantiques** (priorité, retard) et les **actions disponibles** par item.
- Les deux UI ne font que dessiner et renvoyer des actions : JSON via D-Bus sur GNOME, commandes Tauri ailleurs.
- Conséquence : ajouter un état de PR ou une règle de tri ne touche que le cœur, et les deux UI en profitent sans changement.

### Découpage en crates et paquets

- `crates/pigeonnier-core` (lib) : clients GitHub et Todoist, modèle `Item`, routage, tri, cache SQLite, view-models.
- `crates/pigeonnierd` (bin, Linux) : boucle de sync, interface D-Bus, notifications, serveur MCP.
- `apps/gnome` (Rust, Relm4, libadwaita) : client D-Bus du démon.
- `apps/tauri` (Windows, Android) : UI web, cœur embarqué dans le processus.
- `extensions/gnome` (GJS) : indicateur, menu, placement de la sidebar.

### Briques

| Besoin | Crate ou outil |
| --- | --- |
| Async, HTTP | `tokio`, `reqwest` |
| GraphQL typé | `cynic` ou `graphql_client`, schéma GitHub vendorisé |
| Cache | `rusqlite` (SQLite) |
| D-Bus | `zbus` |
| Notifications Linux | `notify-rust` (actions incluses) |
| Secrets | `keyring` |
| MCP | `rmcp` (SDK Rust officiel) |
| UI GNOME | `gtk4-rs`, `libadwaita-rs`, Relm4 |
| UI Windows et Android | Tauri 2, Fluent UI Web Components, Solid ou Lit |

### Distribution

- **GNOME** : Flatpak (Flathub) et `.deb` ; l'extension sur extensions.gnome.org.
- **Windows 11** : installeurs `.msi` x64 et ARM64, construits en CI.
- **Android** : APK signé, publiable sur F-Droid ou installable via Obtainium.

## UI : fenêtre normale et mode sidebar

Une seule liste, **groupée par projet puis triée par urgence**. La sidebar en est la version compacte ; la fenêtre complète ajoute une colonne de projets et les sections par type.

&#91;embedded content: maquette · mode sidebar et fenêtre complète\]

Le drapeau coloré porte la priorité, le liseré la couleur du projet, la ligne grise le contexte et l'échéance. Le filtre « Action » masque tout ce qui est en attente.

### Mode sidebar

- **Format** : 280 à 360 px de large, toute la hauteur de l'écran.
- **Filtres** : Tout · Action requise · Aujourd'hui (en retard compris).
- **Projets** : repliables, état mémorisé ; les projets vides sont masqués.
- **Ligne** : drapeau, titre, contexte et échéance. Au survol ou au glissement : ouvrir, reporter, cocher.
- **Ajout rapide** en bas, dans le projet survolé ou déplié.
- **Placement** : l'extension GNOME colle la fenêtre au bord droit, pleine hauteur, et la bascule avec Super+S. Sans extension, c'est une fenêtre étroite dont la taille est mémorisée.

### Fenêtre complète

- Colonne de projets à gauche avec compteurs, plus une entrée « Tous » qui reprend la vue sidebar en large.
- Pour le projet choisi : sections **PR à traiter**, **Actions manuelles**, **Roadmap**, chacune repliable.
- Panneau de détail à droite au clic : description Todoist, checks et reviewers de la PR, champs du Project.

### La sidebar est un point de rupture, pas une autre vue

Sur GNOME, la sidebar est le point de rupture étroit de la fenêtre libadwaita (`AdwBreakpoint`, `AdwNavigationSplitView`) : il n'y a pas de seconde fenêtre à maintenir. Côté Tauri, le même composant étroit sert le téléphone Android et une fenêtre étroite sur Windows. En mode tactile (Surface, téléphone), les cibles passent à 44 px et les actions se font par glissement.

### Clavier

| Touche | Action |
| --- | --- |
| j / k | élément suivant / précédent |
| Entrée | ouvrir |
| x | cocher (Todoist) |
| s | reporter |
| a | ajout rapide |
| 1 à 9 | aller au projet n |
| / | rechercher |
| r | rafraîchir |

### Thème

Natif sur GNOME : libadwaita suit d'office le mode clair ou sombre et la couleur d'accent. Sur Windows, thème Fluent avec l'accent système et l'effet Mica. Sur Android, couleurs et mode sombre du système.

## Notifications et extension GNOME Shell

Les notifications partent du **démon** (voir Architecture), donc elles fonctionnent fenêtre fermée. L'extension GNOME est un simple client D-Bus de ce démon : elle n'a ni token ni logique de sync.

### Ce qui déclenche une notification

| Événement | Quand | Actions dans la notif |
| --- | --- | --- |
| Nouvelle review demandée | dès la détection | Ouvrir · Reporter 2 h |
| Ma PR passe en « action requise » (CI rouge, changements demandés, à merger) | à la transition d'état | Ouvrir · Reporter 2 h |
| Tâche Todoist avec heure | à l'heure prévue | Fait · Reporter 1 h · Demain |
| Échéance (deadline) | la veille à 18 h, puis le jour J à l'heure du digest | Ouvrir · Reporter |
| Digest du matin | heure réglable, jours ouvrés | Ouvrir la sidebar |

Le digest résume par projet : « helpers4 : 2 reviews, 1 action Claude · Sunny Tech : 1 échéance aujourd'hui ».

### Garde-fous contre le bruit

- Chaque notification est émise **une seule fois** par (item, état) ; l'historique vit dans le cache.
- Les projets en sourdine et les PR de bots ne notifient jamais.
- Plage silencieuse réglable (par défaut 20 h–8 h), en plus du mode Ne pas déranger de GNOME, qui reste respecté.
- Au-delà de 3 notifications dans la même minute, elles fusionnent en une seule (« 5 nouveaux éléments »).

### Extension GNOME Shell

- **Indicateur** dans la barre du haut : une icône et un compteur des éléments P1 ou en retard. Le compteur est rouge s'il y a du retard, neutre sinon, et masqué à zéro.
- **Menu déroulant** : la liste groupée par projet, chaque projet repliable avec sa pastille de couleur, ses 5 premiers éléments et un lien « +3 ».
- **Interactions** : clic sur un élément pour l'ouvrir, case à cocher pour terminer une tâche Todoist, entrée « Ouvrir la sidebar » en bas du menu.
- **Raccourci clavier global** (par ex. Super+S) pour afficher ou masquer la sidebar.
- **Compatibilité** : GJS en modules ESM, donc GNOME 45 et plus (Ubuntu 24.04 et suivantes).

### Interface D-Bus du démon

```text
bus de session · nom : io.github.baxyz.Pigeonnier · objet : /io/github/baxyz/Pigeonnier

interface io.github.baxyz.Pigeonnier1
  method  GetSnapshot() -> s                 # JSON : projets + items triés
  method  Refresh()
  method  Complete(s item_id)                # Todoist seulement
  method  Snooze(s item_id, x until_unix)
  method  ShowWindow(s mode)                 # "sidebar" | "full"
  signal  Changed(u revision)
  property Counts  a{s(uu)}                  # projet -> (action requise, en retard)
```

### Hors GNOME

- **Windows 11** : icône dans la zone de notification avec le même compteur, menu déroulant, notifications natives.
- **Android** : notifications système ; widget d'écran d'accueil en option, plus tard.
- **Autres bureaux** : sur demande uniquement. Pour KDE, un plasmoïde jouerait le rôle de l'extension et de la sidebar.

## Actions : jusqu'où aller

La v1 reste en **lecture, plus deux écritures sans risque** (cocher et reporter une tâche Todoist). Chaque niveau suivant ajoute des écritures plus engageantes ; le niveau 3 fait de Pigeonnier le point de contact entre toi et Claude.

### Niveau 0 : lecture (v1)

- Ouvrir un élément dans le navigateur (ou dans VS Code pour une PR, via l'extension GitHub Pull Requests).
- Report local d'une PR ou d'un item de roadmap.
- Cocher une tâche Todoist ; la reporter (vraie date `due`).

### Niveau 1 : Todoist complet

- **Ajout rapide dans un projet** : champ en bas de chaque projet, langage naturel (« relire la doc vendredi p2 ») envoyé à Todoist, qui l'analyse, dans la bonne section.
- Changer la priorité et l'échéance depuis le menu contextuel.
- Commentaire rapide sur une tâche (utile pour répondre à Claude).

### Niveau 2 : GitHub léger, toujours avec confirmation

- **Merger** une PR à l'état « à merger » (méthode de merge par défaut du repo).
- **Relancer** les jobs en échec d'une CI rouge.
- **Changer le statut** d'un item de roadmap (champ single-select du Project).
- **S'assigner** un item de roadmap.
- Pas d'approbation de PR depuis Pigeonnier : une review doit passer par la lecture du diff.

### Niveau 3 : pont avec Claude

- **Liens PR ↔ tâche** : une tâche Todoist dont la description contient l'URL d'une PR s'affiche sous cette PR. Option : cocher la tâche automatiquement quand la PR est mergée.
- **Pigeonnier comme serveur MCP** : le démon expose quelques outils à Claude, en local sur ligno et fajro :
  - `list_items(project?)` : ce qui t'attend, pour que Claude priorise avec toi ;
  - `add_manual_action(project, title, due?, priority?, pr_url?)` : crée la tâche Todoist dans la bonne section avec le label `claude`. La convention n'a plus besoin d'être rappelée à Claude, l'outil l'applique.
- **« Confier à Claude »** sur une PR « CI rouge » ou « à corriger » : lance une session Claude Code sur fajro dans le devcontainer du repo, avec l'URL de la PR comme contexte. Le résultat revient sous forme de commit sur la PR, et éventuellement d'une action manuelle si Claude a besoin de toi.

Le niveau 3 est le seul qui crée vraiment de la valeur au-delà d'un agrégateur. C'est aussi le plus incertain techniquement : à prototyper isolément avant de l'intégrer.

## Roadmap de développement

Six jalons, dont **trois suffisent pour un outil utilisé au quotidien** (M0 à M2). Chaque jalon se termine par une porte : on ne passe au suivant que si son critère est rempli.

&#91;embedded content: roadmap · 6 jalons, 5 portes\]

M1 vaut d'être fait même seul : une CLI `pigeonnier list` sert dès le premier soir sur fajro, et elle valide le cœur avant toute UI. Hors périmètre, **sur demande uniquement** : KDE (plasmoïde QML dans un panneau vertical), iOS, macOS.

### Organisation du dépôt

- Monorepo : `crates/pigeonnier-core`, `crates/pigeonnierd`, `apps/gnome (Relm4), apps/tauri (Windows, Android)`, `extensions/gnome`.
- Devcontainer basé sur tes features helpers4 (Rust, Node, `claude-dev`), pour que Claude puisse travailler dessus sur fajro.
- CI GitHub Actions : tests du cœur contre des réponses GitHub et Todoist enregistrées (aucun token en CI), builds Linux et Windows ARM64, puis Android en M5.
- Suivi du projet dans… Pigeonnier, une fois M2 atteint.

## Risques et questions ouvertes

Le risque le plus sous-estimé est **le mode sidebar sous Wayland** : une application ne peut ni se positionner ni se coller au bord de l'écran sous GNOME. Seule l'extension Shell peut le faire.

### Risques

| Risque | Impact | Parade |
| --- | --- | --- |
| Wayland interdit à l'app de placer sa fenêtre | Sidebar flottante au lieu d'être collée au bord | L'extension GNOME place et redimensionne la fenêtre (le Shell en a le droit) ; sans extension, simple fenêtre étroite |
| Deux UI à maintenir (libadwaita et Tauri) | Chaque fonctionnalité visible écrite deux fois | View-models dans le cœur, UI sans logique métier |
| Tauri mobile moins mûr pour l'arrière-plan | Sync et notifications Android fragiles | Petit plugin Kotlin (WorkManager) pour la sync périodique |
| OAuth Todoist sans PKCE | Client secret embarqué dans une app open source | Vérifier la doc ; sinon token personnel collé par défaut |
| Reviews demandées à une team entière | Bruit dans « à reviewer » | Option : qualificatif `user-review-requested:@me` (demandes directes uniquement) |
| Projects v2 aux champs hétérogènes | Priorités ou dates mal lues | Mapping des champs par projet, valeurs par défaut du template GitHub |
| Android limite le travail en arrière-plan | Notifications retardées de 15 min ou plus | Sync à l'ouverture + tâche périodique ; pas de push sans serveur |
| Plafond de 1 000 résultats de l'API Search | Aucun en pratique pour des PR ouvertes | Rien à faire, à surveiller |

### Questions ouvertes

- [ ] **Où tourne le démon ?** Sur ligno (simple), ou sur fajro, toujours allumé, avec ligno, la Surface et le téléphone comme clients légers via le réseau local ou un VPN ? Fajro permettrait des notifications même PC éteint et un seul cache.
- [ ] **Label des actions Claude** : `claude` convient-il, et Claude le pose-t-il déjà via le plugin Todoist ?
- [ ] **Périmètre des PR** : inclure les PR assignées sans être auteur ? Les demandes de review adressées à une team ?
- [ ] **Projet Todoist parent** : un seul projet (« Dev ») avec une section par org, ou plusieurs projets Todoist ?
- [ ] **Nom** : tranché, ce sera Pigeonnier, et pigeonnier pour les identifiants techniques (crates, démon, CLI, D-Bus). L'org GitHub pigeonnier étant prise, le dépôt démarre en baxyz/pigeonnier.
- [ ] **Licence** : AGPL comme Folioscope, ou GPL/MIT pour faciliter une diffusion sur Flathub et F-Droid ?
