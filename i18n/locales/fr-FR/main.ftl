## Catalogue Fluent fr-FR (français métropolitain).
##
## DRAFT — Première traduction non révisée. Les locuteurs natifs
## sont invités à proposer des corrections via une PR; les
## changements sont des modifications de ce fichier uniquement,
## aucun code à toucher.
##
## Convention: tutoiement non utilisé. L'interface s'adresse aux
## utilisateurs à la deuxième personne du pluriel ("vous") comme
## il est d'usage dans les applications professionnelles
## francophones.

# Generic
greeting = Bonjour, { $name }.
unread-count = { $count ->
    [0] Aucun nouveau message.
    [one] Un nouveau message.
   *[other] { $count } nouveaux messages.
}

# Transactional email subjects.
password-reset-subject = Réinitialiser votre mot de passe { $app }
invitation-subject = Vous avez été invité à rejoindre { $app } - Créez votre compte
# Invitation email body.
invitation-title = Bienvenue sur { $app } !
invitation-greeting = Bonjour <strong>{ $name }</strong>,
invitation-intro = Vous avez été invité à rejoindre <strong>{ $app }</strong> par <strong>{ $by }</strong>.
invitation-action-prompt = Pour finaliser la création de votre compte et choisir votre mot de passe, cliquez sur le bouton ci-dessous :
invitation-cta-label = Configurer mon compte
invitation-notice-expiry = Ce lien d'invitation expirera dans <strong>7 jours</strong>
invitation-notice-create-password = Vous devrez choisir un mot de passe pendant la configuration
invitation-notice-strong-password = Choisissez un mot de passe robuste d'au moins 8 caractères
invitation-notice-unexpected = Si vous n'attendiez pas cette invitation, vous pouvez ignorer cet e-mail
invitation-footer = Pour toute question, contactez votre administrateur système.
invitation-body-text =
    Bonjour { $name },

    Vous avez été invité à rejoindre { $app } par { $by }.

    Pour finaliser la création de votre compte et choisir votre mot de passe, ouvrez ce lien dans votre navigateur :

    { $link }

    À savoir :
      - Cette invitation expirera dans 7 jours.
      - Vous choisirez un mot de passe pendant la configuration.
      - Choisissez un mot de passe robuste d'au moins 8 caractères.
      - Si vous n'attendiez pas cette invitation, vous pouvez ignorer cet e-mail.

    -- { $app }

# Notification email subjects.
notif-ticket-assigned = [{ $app }] Ticket assigné : { $title }
notif-ticket-status-changed = [{ $app }] Statut modifié : { $title }
notif-comment-added = [{ $app }] Nouveau commentaire : { $title }
notif-mentioned = [{ $app }] { $actor } vous a mentionné
notif-ticket-created-requester = [{ $app }] Ticket créé : { $title }
notif-doc-page-updated = [{ $app }] Page mise à jour : { $title }
# Notification email body.
notif-body-fallback = Vous avez une nouvelle notification.
notif-from-row = <strong>De :</strong> { $actor }
notif-cta-view-in = Ouvrir dans { $app }
notif-footer-preferences = Vous recevez cet e-mail en raison de vos préférences de notification.
notif-body-text =
    { $title }

    { $body }

    De : { $actor }

    Ouvrir dans { $app } : { $cta }

    -- Vous recevez cet e-mail en raison de vos préférences de notification dans { $app }.

# Login + MFA challenge view.
login-subtitle = Connectez-vous à votre compte
login-email-label = E-mail
login-email-placeholder = Saisissez votre e-mail
login-password-label = Mot de passe
login-password-placeholder = Saisissez votre mot de passe
login-password-show = Afficher le mot de passe
login-password-hide = Masquer le mot de passe
login-forgot-password = Mot de passe oublié ?
login-submit = Se connecter
login-submitting = Connexion...
login-passkey-cta = Se connecter avec une clé d'accès
login-passkey-authenticating = Authentification...
login-microsoft-cta = Se connecter avec Microsoft Entra
login-microsoft-connecting = Connexion...
login-microsoft-logout-title = Se déconnecter du compte Microsoft
login-oidc-cta = Se connecter avec { $provider }
login-oidc-logout-title = Se déconnecter du compte { $provider }
login-oidc-connecting = Connexion...
login-divider-or = ou
login-mfa-title = Authentification à deux facteurs
login-mfa-subtitle = Veuillez saisir votre code d'authentification
login-mfa-code-label = Code d'authentification
login-mfa-code-help = Saisissez le code à 6 chiffres de votre application d'authentification ou un code de secours à 8 caractères
login-mfa-back = Retour
login-mfa-verify = Vérifier et se connecter
login-mfa-verifying = Vérification...
login-passkey-mfa-verified = Mot de passe vérifié pour { $email }
login-passkey-mfa-verify-cta = Vérifier avec une clé d'accès
login-passkey-mfa-use-recovery = Utiliser un code de secours
login-passkey-mfa-back-to-login = Retour à la connexion
login-recovery-code-label = Code de secours
login-recovery-code-placeholder = Saisissez le code de secours
login-recovery-code-help = Saisissez l'un des codes de secours à 8 caractères enregistrés lors de la configuration

# Forgot-password modal.
forgot-password-title = Réinitialiser votre mot de passe
forgot-password-close-modal = Fermer la fenêtre
forgot-password-intro = Saisissez votre adresse e-mail et nous vous enverrons un lien pour réinitialiser votre mot de passe.
forgot-password-email-label = Adresse e-mail
forgot-password-email-placeholder = vous@exemple.com
forgot-password-cancel = Annuler
forgot-password-submit = Envoyer le lien
forgot-password-submitting = Envoi...
forgot-password-error-default = Échec de l'envoi de l'e-mail. Veuillez réessayer.
forgot-password-success-title = Vérifiez votre boîte mail
forgot-password-success-body = Si un compte existe avec cette adresse, nous avons envoyé un lien de réinitialisation à { $email }
forgot-password-success-important = Important :
forgot-password-success-tip-expiry = Le lien expirera dans <strong>1 heure</strong>
forgot-password-success-tip-spam = Consultez votre dossier spam si vous ne le voyez pas
forgot-password-success-tip-close = Vous pouvez fermer cette fenêtre
forgot-password-success-done = Terminé

# Profile settings tabs.
settings-tab-profile = Profil
settings-tab-appearance = Apparence
settings-tab-language = Langue
settings-tab-notifications = Notifications
settings-tab-security = Sécurité
settings-sidebar-heading = Paramètres
settings-subtitle = Gérez votre profil, vos préférences et vos paramètres de sécurité
settings-loading-user = Chargement des paramètres utilisateur...
settings-user-heading = Paramètres utilisateur
settings-section-suffix = - Paramètres

# Dashboard.
dashboard-greeting-morning = Bonjour { $name }.
dashboard-greeting-afternoon = Bon après-midi { $name }.
dashboard-greeting-evening = Bonsoir { $name }.
dashboard-greeting-late-night = Bonsoir { $name }, il se fait tard.
dashboard-subtitle = Bienvenue sur votre tableau de bord { $app }
dashboard-edit-button = Modifier le tableau de bord
dashboard-guest-fallback = Invité

# États vides des principales listes.
empty-documentation-grid-title = Aucune documentation pour le moment
empty-documentation-grid-description = Créez votre première page de documentation pour commencer.
empty-documentation-index-title = Démarrez votre base de connaissances
empty-documentation-index-description = Les pages de documentation permettent à votre équipe de centraliser procédures, FAQ et politiques. Créez la première page pour commencer.
empty-documentation-archived-title = Aucune page archivée
empty-documentation-archived-description = Les pages archivées apparaîtront ici.
empty-documentation-trash-title = La corbeille est vide
empty-documentation-trash-description = Les pages supprimées apparaîtront ici.
empty-project-search-title = Aucun projet trouvé
empty-project-search-description = Essayez d'ajuster vos critères de recherche
empty-project-available-title = Aucun projet disponible
empty-project-available-description = Créez un projet pour commencer
empty-device-search-prompt-title = Rechercher un appareil
empty-device-search-prompt-description = Commencez à taper pour trouver des appareils par nom, numéro de série ou utilisateur
empty-device-search-title = Aucun appareil trouvé
empty-device-search-description = Essayez d'ajuster vos critères de recherche
empty-users-default-title = Aucun utilisateur trouvé
empty-users-default-description = Invitez des utilisateurs pour commencer
empty-users-search-title = Aucun utilisateur ne correspond
empty-users-search-description = Essayez d'ajuster vos critères de recherche
empty-devices-default-title = Aucun appareil trouvé
empty-devices-default-description = Ajoutez votre premier appareil pour commencer
empty-devices-search-title = Aucun appareil ne correspond
empty-devices-search-description = Essayez d'ajuster votre recherche ou vos filtres
empty-groups-title = Aucun groupe pour le moment
empty-groups-description = Créez votre premier groupe pour organiser les utilisateurs
empty-assignment-rules-title = Aucune règle d'attribution
empty-assignment-rules-description = Créez votre première règle pour attribuer automatiquement les tickets
empty-webhooks-title = Aucun webhook
empty-webhooks-description = Créez un webhook pour envoyer des événements à des services externes
empty-api-tokens-title = Aucun jeton API
empty-api-tokens-description = Créez un jeton API pour activer l'accès programmatique à l'API
empty-categories-title = Aucune catégorie
empty-categories-description = Créez des catégories pour organiser les tickets
empty-plugins-installed-title = Aucun plugin installé
empty-plugins-installed-description = Les plugins étendent { $app } avec des intégrations et fonctionnalités personnalisées. Parcourez le registre pour une installation en un clic.

# Persistent shell.
nav-group-work = Travail
nav-group-resources = Ressources
nav-dashboard = Tableau de bord
nav-tickets = Tickets
nav-cycles = Cycles
nav-projects = Projets
nav-devices = Appareils
nav-assets = Ressources matérielles
nav-users = Utilisateurs
nav-documentation = Documentation
nav-inbox = Boîte de réception
nav-collapse = Réduire
nav-search = Rechercher
nav-more = Plus
nav-toggle-sidebar = Basculer la barre latérale
nav-secondary = Navigation secondaire
user-menu-aria = Menu utilisateur
user-menu-view-profile = Voir le profil
user-menu-account = Compte
user-menu-administration = Administration
user-menu-sign-out = Se déconnecter
user-menu-guest-name = Invité

# Tickets — états vides + barre d'actions groupées.
ticket-list-empty-no-assigned-message = Aucun ticket ne vous est assigné.
ticket-list-empty-showing-all-active = Affichage de tous les tickets actifs à la place.
ticket-list-empty-no-match-title = Aucun ticket ne correspond.
ticket-list-empty-no-match-description = Retirez des filtres pour en voir plus.
ticket-list-empty-triage-clear-title = Triage terminé.
ticket-list-empty-triage-clear-description = Les nouveaux tickets à catégoriser apparaîtront ici.
ticket-list-empty-all-caught-up-title = Tout est à jour.
ticket-list-empty-all-caught-up-description = Aucun ticket ouvert ne vous est assigné.
ticket-list-empty-no-active-title = Aucun ticket actif.
ticket-list-empty-no-active-description = Tous les tickets sont résolus ou annulés.
ticket-list-empty-no-in-view-title = Aucun ticket dans cette vue.
ticket-list-empty-no-in-view-description = Ajustez le filtre ou choisissez une autre vue.
ticket-list-bulk-actions-aria = Actions groupées
ticket-list-bulk-status = Statut
ticket-list-bulk-priority = Priorité
ticket-list-bulk-assign = Assigner
ticket-list-bulk-clear-title = Effacer la sélection (Échap)
ticket-list-bulk-clear = Effacer
ticket-list-row-density-aria = Densité des lignes
ticket-list-save-view-title = Enregistrer l'état actuel comme vue privée
ticket-list-recurring-title = Ticket récurrent
ticket-list-sla-breached-title = SLA dépassée

# Détail du ticket.
ticket-detail-reconnecting-title = Reconnexion aux mises à jour en direct
ticket-detail-connecting = Connexion...
ticket-detail-more-actions = Plus d'actions
ticket-detail-section-details = Détails du ticket
ticket-detail-section-notes = Notes du ticket
ticket-detail-section-comments = Commentaires et pièces jointes
ticket-detail-prop-title = Titre
ticket-detail-prop-requester = Demandeur
ticket-detail-prop-assignee = Assigné à
ticket-detail-prop-status = Statut
ticket-detail-prop-priority = Priorité
ticket-detail-prop-category = Catégorie
ticket-detail-prop-created = Créé
ticket-detail-prop-last-modified = Dernière modification
ticket-detail-delete-title = Supprimer le ticket
ticket-detail-delete-confirm-heading = Supprimer ce ticket ?
ticket-detail-delete-confirm-body = Cette action est irréversible. Le ticket et son historique seront supprimés.
ticket-detail-delete-cancel = Annuler
ticket-detail-delete-confirm = Supprimer

# Settings.
settings-localization-title = Langue et fuseau horaire
settings-localization-help = Détermine la langue des messages et l'affichage des dates. La valeur par défaut du site s'applique si rien n'est sélectionné.
settings-language-label = Langue
settings-timezone-label = Fuseau horaire
settings-locale-site-default = Par défaut du site
settings-locale-en-US = Anglais (États-Unis)
settings-locale-en-GB = Anglais (Royaume-Uni)
settings-locale-en-AU = Anglais (Australie)
settings-locale-fr-FR = Français (France)
settings-locale-nl-NL = Néerlandais (Pays-Bas)
settings-timezone-browser-detected = Détecté par le navigateur ({ $tz })
settings-timezone-use-device = Utiliser le fuseau de l'appareil
settings-timezone-search-placeholder = Rechercher une ville ou un décalage (ex. Paris, UTC+1)
settings-timezone-no-matches = Aucun fuseau ne correspond
settings-save = Enregistrer
settings-saving = Enregistrement...
settings-localization-saved = Préférences de langue et de fuseau enregistrées
settings-localization-save-failed = Échec de l'enregistrement des préférences

# Channel auto-acknowledgement.
auto-ack-default-template = Votre demande (#{ $ticket_id }) a été reçue et est en cours d'examen par notre équipe d'assistance. Pour ajouter d'autres commentaires, répondez à cet e-mail.

# Inbox-time connecting copy.
inbox-time-just-now = À l'instant
inbox-time-yesterday = Hier à { $time }
inbox-time-weekday = { $day } à { $time }

# Password-reset email body.
password-reset-title = Demande de réinitialisation du mot de passe
password-reset-greeting = Bonjour <strong>{ $name }</strong>,
password-reset-intro = Nous avons reçu une demande de réinitialisation du mot de passe de votre compte <strong>{ $app }</strong>. Si vous n'êtes pas à l'origine de cette demande, vous pouvez ignorer cet e-mail.
password-reset-action-prompt = Pour réinitialiser votre mot de passe, cliquez sur le bouton ci-dessous :
password-reset-cta-label = Réinitialiser le mot de passe
password-reset-notice-expiry = Ce lien expirera dans <strong>1 heure</strong>
password-reset-notice-single-use = Ce lien ne peut être utilisé qu'<strong>une seule fois</strong>
password-reset-notice-never-share = Ne partagez jamais ce lien avec personne
password-reset-notice-account-security = Si vous n'avez pas demandé cette réinitialisation, sécurisez immédiatement votre compte
password-reset-footer = Pour toute question, contactez votre administrateur système.
password-reset-body-text =
    Bonjour { $name },

    Nous avons reçu une demande de réinitialisation du mot de passe de votre compte { $app }. Si vous n'êtes pas à l'origine de cette demande, vous pouvez ignorer cet e-mail.

    Pour réinitialiser votre mot de passe, ouvrez ce lien dans votre navigateur :

    { $link }

    Notes de sécurité :
      - Ce lien expirera dans 1 heure.
      - Ce lien ne peut être utilisé qu'une seule fois.
      - Ne partagez jamais ce lien avec personne.
      - Si vous n'avez pas demandé cette réinitialisation, sécurisez votre compte.

    Pour toute question, contactez votre administrateur système.

    -- { $app }

# Onboarding administrateur (premier démarrage).
onboarding-welcome-title = Bienvenue dans Nosdesk
onboarding-welcome-subtitle = Commençons par créer votre compte administrateur
onboarding-error-setup-status = Impossible de vérifier l'état de l'installation. Veuillez réessayer.
onboarding-success-logging-in = Compte administrateur créé. Connexion en cours...
onboarding-success-fallback = Compte créé avec succès. Veuillez vous connecter avec vos identifiants.
onboarding-success-fallback-redirect = Compte créé avec succès. Veuillez vous connecter.
onboarding-error-setup-failed = L'installation a échoué. Veuillez réessayer.
onboarding-error-unexpected = Une erreur inattendue s'est produite. Veuillez réessayer.
onboarding-validation-token = Le jeton d'amorçage est requis
onboarding-validation-name = Le nom de l'administrateur est requis
onboarding-validation-email = L'adresse e-mail est requise
onboarding-validation-email-format = Veuillez saisir une adresse e-mail valide
onboarding-validation-password-length = Le mot de passe doit comporter au moins 8 caractères
onboarding-validation-password-mismatch = Les mots de passe ne correspondent pas
onboarding-token-label = Jeton d'amorçage
onboarding-token-placeholder = Collez le jeton à usage unique du serveur
onboarding-token-hint = Consultez les journaux de démarrage du serveur pour l'URL d'installation, ou récupérez-le manuellement avec
onboarding-name-label = Nom de l'administrateur
onboarding-name-placeholder = Saisissez votre nom complet
onboarding-email-label = Adresse e-mail
onboarding-email-placeholder = Saisissez votre adresse e-mail
onboarding-password-label = Mot de passe
onboarding-password-placeholder = Choisissez un mot de passe sécurisé (8 caractères minimum)
onboarding-confirm-password-label = Confirmer le mot de passe
onboarding-confirm-password-placeholder = Confirmez votre mot de passe
onboarding-submit = Créer le compte administrateur
onboarding-submit-loading = Création de l'administrateur...
onboarding-progress-title = Configuration de votre compte
onboarding-progress-subtitle = Cela ne prendra qu'un instant...
onboarding-complete-title = Bienvenue dans Nosdesk
onboarding-complete-subtitle = Votre compte administrateur est prêt.
onboarding-migration-title = Migrer depuis une autre instance Nosdesk ?
onboarding-migration-body-prefix = Créez un administrateur ici, puis exécutez
onboarding-migration-body-suffix = sur l'hôte. La restauration remplace l'administrateur par les utilisateurs importés.
onboarding-security-title = Avis de sécurité
onboarding-security-body = Cela crée le premier compte administrateur de votre installation Nosdesk. Choisissez un mot de passe fort ; ce compte aura un accès complet au système.

# Assistant de configuration MFA.
mfa-setup-header-default = Finalisez la configuration de votre compte
mfa-setup-header-offer = Ajouter une autre méthode ?
mfa-setup-header-additional = Ajouter une méthode de secours
mfa-setup-subtitle-default = Votre type de compte exige une authentification multifacteur pour la sécurité
mfa-setup-subtitle-choose = Choisissez votre méthode d'authentification préférée
mfa-setup-subtitle-offer-passkey = Les passkeys offrent une connexion plus rapide et sans mot de passe
mfa-setup-subtitle-offer-totp = Une application d'authentification sert de secours si vous perdez votre passkey
mfa-setup-subtitle-passkey-additional = Configurez une passkey pour une connexion plus rapide
mfa-setup-subtitle-totp-additional = Configurez une application d'authentification en tant que secours
mfa-setup-totp-name = Application d'authentification
mfa-setup-totp-description = Utilisez une application comme Google Authenticator, Authy ou 1Password pour générer des codes temporels
mfa-setup-passkey-name = Passkey
mfa-setup-passkey-description = Utilisez la biométrie (Face ID, Touch ID) ou une clé de sécurité matérielle pour une connexion sans mot de passe
mfa-setup-which-title = Laquelle choisir ?
mfa-setup-which-passkey-label = Les passkeys
mfa-setup-which-passkey-body = sont plus sûres et plus pratiques, utilisez simplement votre empreinte ou votre visage.
mfa-setup-which-totp-label = Les applications d'authentification
mfa-setup-which-totp-body = fonctionnent sur tout appareil et ne nécessitent pas la biométrie.
mfa-setup-totp-success-title = Application d'authentification configurée !
mfa-setup-totp-success-body = Souhaitez-vous également ajouter une passkey pour une connexion plus rapide et sans mot de passe ?
mfa-setup-passkey-success-title = Passkey créée !
mfa-setup-passkey-success-body = Souhaitez-vous également configurer une application d'authentification comme méthode de secours ?
mfa-setup-add-passkey-title = Ajouter une passkey
mfa-setup-add-passkey-description = Utilisez Face ID, Touch ID ou une clé de sécurité
mfa-setup-add-totp-title = Configurer une application d'authentification
mfa-setup-add-totp-description = À utiliser en secours si vous perdez l'accès à votre passkey
mfa-setup-skip-now = Ignorer pour l'instant
mfa-setup-back-to-login = Retour à la connexion
mfa-setup-back-skip = Ignorer
mfa-setup-back-different = Choisir une autre méthode
mfa-setup-error-session-expired = Session expirée. Veuillez vous reconnecter pour configurer la MFA.
mfa-setup-error-invalid-access = Accès invalide. Redirection vers la connexion...

# Réinitialisation du mot de passe.
password-reset-title = Réinitialiser votre mot de passe
password-reset-subtitle = Saisissez votre nouveau mot de passe ci-dessous
password-reset-success-title = Réinitialisation terminée !
password-reset-success-body = Votre mot de passe a été mis à jour. Vous pouvez vous connecter avec le nouveau mot de passe.
password-reset-success-cta = Aller à la connexion
password-reset-field-new = Nouveau mot de passe
password-reset-field-new-placeholder = Saisissez le nouveau mot de passe
password-reset-field-confirm = Confirmer le nouveau mot de passe
password-reset-field-confirm-placeholder = Confirmez le nouveau mot de passe
password-reset-req-length = Au moins 8 caractères
password-reset-match-yes = Les mots de passe correspondent
password-reset-match-no = Les mots de passe ne correspondent pas
password-reset-submit = Réinitialiser le mot de passe
password-reset-submit-loading = Réinitialisation...
password-reset-back-to-login = Retour à la connexion
password-reset-error-no-token = Jeton de réinitialisation invalide ou manquant. Veuillez demander une nouvelle réinitialisation.
password-reset-error-failed = Échec de la réinitialisation. Le lien a peut-être expiré.

# Acceptation d'invitation / ticket invité.
accept-invitation-heading-validating = Un instant…
accept-invitation-heading-guest = Confirmer l'envoi de votre ticket
accept-invitation-heading-welcome = Bienvenue dans { $app }
accept-invitation-subheading-validating = Vérification de votre lien.
accept-invitation-subheading-guest = Définissez un mot de passe pour valider votre ticket.
accept-invitation-subheading-invitation = Finalisez la configuration de votre compte.
accept-invitation-checking = Vérification de votre lien…
accept-invitation-invalid-title-guest = Ce lien de confirmation n'est plus valide
accept-invitation-invalid-title-invitation = Invitation invalide
accept-invitation-go-to-signin = Aller à la connexion
accept-invitation-activating-title-guest = Validation de votre ticket…
accept-invitation-activating-title-invitation = Activation de votre compte…
accept-invitation-signing-in = Connexion en cours…
accept-invitation-success-title-guest = Tout est prêt
accept-invitation-success-title-invitation = Bienvenue dans { $app }
accept-invitation-manual-login = Connectez-vous avec le mot de passe que vous venez de définir.
accept-invitation-password-label = Mot de passe
accept-invitation-password-placeholder = Au moins 8 caractères
accept-invitation-confirm-label = Confirmer le mot de passe
accept-invitation-confirm-placeholder = Saisissez-le à nouveau
accept-invitation-req-length = Au moins 8 caractères
accept-invitation-match-yes = Les mots de passe correspondent
accept-invitation-match-no = Les mots de passe ne correspondent pas
accept-invitation-show-password = Afficher le mot de passe
accept-invitation-hide-password = Masquer le mot de passe
accept-invitation-submit-guest = Confirmer et valider le ticket
accept-invitation-submit-loading-guest = Confirmation…
accept-invitation-submit-invitation = Activer le compte
accept-invitation-submit-loading-invitation = Activation…
accept-invitation-back-to-signin = Retour à la connexion
accept-invitation-error-missing-token = Lien de confirmation invalide ou manquant.
accept-invitation-error-default = Ce lien est invalide ou a expiré.
accept-invitation-error-validation-failed = Échec de la validation du lien. Veuillez réessayer plus tard.
accept-invitation-error-submit = Échec de la confirmation. Le lien a peut-être expiré.

# Admin : journal d'audit.
admin-audit-title = Journal d'audit
admin-audit-description = Trace de qui a modifié quoi sur les entités auditées. Par défaut, les 7 derniers jours et les 50 entrées les plus récentes ; affinez avec les filtres ci-dessous.
admin-audit-filter-entity = Entité
admin-audit-filter-any = Toutes
admin-audit-filter-entity-id = ID d'entité
admin-audit-filter-entity-id-placeholder = ex. 42
admin-audit-filter-actor = UUID de l'acteur
admin-audit-filter-actor-placeholder = ex. 0192…
admin-audit-clear-filters = Effacer les filtres
admin-audit-empty-title = Aucune entrée d'audit
admin-audit-empty-description = Aucune entité auditée n'a changé sur la période sélectionnée, ou les filtres excluent toutes les lignes.
admin-audit-by = par
admin-audit-corr = corr
admin-audit-diff-field = Champ
admin-audit-diff-old = Ancien
admin-audit-diff-new = Nouveau
admin-audit-no-diff = Pas de diff au niveau du champ pour cette entrée.
admin-audit-op-created = Créé
admin-audit-op-updated = Mis à jour
admin-audit-op-deleted = Supprimé
admin-audit-actor-system = système
admin-audit-load-more = Charger plus
admin-audit-loading-more = Chargement…
admin-audit-error-load = Échec du chargement du journal d'audit
admin-audit-error-load-more = Échec du chargement de plus d'entrées

# Admin : liste de suppression d'e-mails.
admin-suppressions-title = Liste de suppression d'e-mails
admin-suppressions-description = Adresses auxquelles nous ne tenterons pas de livrer. Les bounces durs (SMTP 5xx / 5.x.x) y arrivent automatiquement ; ajoutez manuellement pour les blocages liés à la conformité ou aux plaintes. Les bounces souples (4xx, transitoires) ne suppriment jamais automatiquement.
admin-suppressions-count-singular = suppression
admin-suppressions-count-plural = suppressions
admin-suppressions-add-title = Ajouter une suppression
admin-suppressions-add-email-placeholder = utilisateur@exemple.com
admin-suppressions-add-note-placeholder = Note facultative (demande de conformité, etc.)
admin-suppressions-adding = Ajout…
admin-suppressions-add = Ajouter
admin-suppressions-empty-title = Aucune suppression
admin-suppressions-empty-description = Les destinataires en bounce dur et les adresses ajoutées manuellement apparaîtront ici.
admin-suppressions-bounce-count-title = Bounce { $count } fois
admin-suppressions-remove = Retirer
admin-suppressions-confirm-title = Retirer de la liste de suppression ?
admin-suppressions-confirm-message = Les envois futurs vers cette adresse seront tentés normalement. Si l'échec initial était un bounce dur, ils échoueront probablement et seront resupprimés.
admin-suppressions-confirm-keep = Garder supprimé
admin-suppressions-load-more = Charger plus
admin-suppressions-loading-more = Chargement…
admin-suppressions-error-load = Échec du chargement des suppressions
admin-suppressions-error-load-more = Échec du chargement de plus
admin-suppressions-error-add = Échec de l'ajout de la suppression
admin-suppressions-error-remove = Échec du retrait
admin-suppressions-reason-hard-bounce = bounce dur
admin-suppressions-reason-manual = manuelle

# Admin : file d'attente des e-mails sortants.
admin-email-queue-title = File d'attente des e-mails sortants
admin-email-queue-description = Trace durable de chaque réponse que nous avons tenté d'envoyer. Le worker vide les lignes en attente toutes les quelques secondes ; les envois échoués réessaient avec un backoff exponentiel. Utilisez cette vue pour investiguer pourquoi une notification n'est pas partie.
admin-email-queue-stat-pending = En attente
admin-email-queue-stat-oldest = Plus ancien : { $age }
admin-email-queue-stat-sent = Envoyé
admin-email-queue-stat-failed = Échoué (réessai en cours)
admin-email-queue-stat-dead = Mort (sans réessai)
admin-email-queue-filter-status = Statut
admin-email-queue-filter-ticket = ID de ticket
admin-email-queue-filter-ticket-placeholder = 42
admin-email-queue-filter-domain = Domaine du destinataire
admin-email-queue-filter-domain-placeholder = exemple.com
admin-email-queue-clear-filters = Effacer les filtres
admin-email-queue-status-pending = en attente
admin-email-queue-status-sending = envoi
admin-email-queue-status-sent = envoyé
admin-email-queue-status-failed = échoué
admin-email-queue-status-dead = mort
admin-email-queue-status-suppressed = supprimé
admin-email-queue-empty-title = Aucun e-mail sortant
admin-email-queue-empty-description = Aucune réponse n'a été envoyée récemment, ou les filtres excluent toutes les lignes.
admin-email-queue-bounced = Bounce
admin-email-queue-bounced-with-diagnostic = Bounce : { $diagnostic }
admin-email-queue-bounced-no-diagnostic = Bounce (aucun diagnostic en amont capturé)
admin-email-queue-attempts-title = { $count } tentative(s)
admin-email-queue-retry-now = Réessayer maintenant
admin-email-queue-cancel = Annuler
admin-email-queue-details = Détails
admin-email-queue-hide = Masquer
admin-email-queue-field-recipient = Destinataire
admin-email-queue-field-channel = Canal
admin-email-queue-field-ticket = Ticket
admin-email-queue-field-comment = Commentaire
admin-email-queue-field-next-attempt = Prochaine tentative
admin-email-queue-field-sent-at = Envoyé le
admin-email-queue-field-failed-at = Échoué le
admin-email-queue-field-smtp-code = Code SMTP
admin-email-queue-field-last-error = Dernière erreur
admin-email-queue-field-bounced-at = Bounce le
admin-email-queue-field-bounce-recipient = Destinataire du bounce
admin-email-queue-field-bounce-reason = Raison du bounce
admin-email-queue-load-more = Charger plus
admin-email-queue-loading-more = Chargement…
admin-email-queue-confirm-title = Annuler l'e-mail en file d'attente ?
admin-email-queue-confirm-message = L'e-mail sera marqué comme supprimé et ne sera pas envoyé.
admin-email-queue-confirm-yes = Annuler l'envoi
admin-email-queue-confirm-no = Conserver
admin-email-queue-error-load = Échec du chargement de la file d'attente
admin-email-queue-error-load-more = Échec du chargement de plus d'entrées
admin-email-queue-error-stats = Échec du chargement des statistiques
admin-email-queue-error-retry = Échec du réessai
admin-email-queue-error-cancel = Échec de l'annulation

# Admin : états du workflow.
admin-workflow-states-title = Workflow
admin-workflow-states-description = Ajoutez des états de ticket nommés dans les catégories de workflow standard. Les catégories sont fixes pour que la SLA, les tableaux de bord et l'automatisation fonctionnent de manière cohérente entre les équipes. Les nouveaux tickets démarrent dans l'état marqué comme défaut.
admin-workflow-states-count-singular = état
admin-workflow-states-count-plural = états
admin-workflow-states-default-badge = Défaut
admin-workflow-states-make-default = Définir par défaut
admin-workflow-states-archive-title = Archiver l'état
admin-workflow-states-archive-disabled-title = Impossible d'archiver l'état par défaut
admin-workflow-states-archive-confirm = Archiver « { $name } » ? Les tickets existants conserveront cet état.
admin-workflow-states-empty-category = Aucun état dans cette catégorie.
admin-workflow-states-add-placeholder = Ajouter un nom d'état
admin-workflow-states-add = Ajouter
admin-workflow-states-error-name-required = Le nom est requis
admin-workflow-states-error-load = Échec du chargement des états de workflow
admin-workflow-states-error-save = Échec de l'enregistrement de l'état
admin-workflow-states-error-default = Échec de la définition par défaut
admin-workflow-states-error-archive = Échec de l'archivage de l'état
admin-workflow-states-error-promote-first = Promouvez un autre état comme défaut avant d'archiver celui-ci.
admin-workflow-states-error-create = Échec de la création de l'état
admin-workflow-states-saved = Enregistré
admin-workflow-states-default-flash = { $name } est désormais l'état par défaut pour les nouveaux tickets
admin-workflow-states-archived-flash = { $name } archivé
admin-workflow-states-added-flash = { $name } ajouté à { $category }

# Habillage de l'administration.
admin-back-to-dashboard = Retour au tableau de bord
admin-heading = Administration
admin-search-placeholder = Rechercher dans les paramètres...
admin-search-empty = Aucun paramètre ne correspond à « { $query } »
admin-clear-search = Effacer la recherche
admin-index-subtitle = Gérez vos paramètres système, intégrations et la configuration de l'espace de travail

admin-nav-group-tickets = Tickets et workflow
admin-nav-group-integrations = Intégrations
admin-nav-group-compliance = Conformité
admin-nav-group-appearance = Apparence et notifications
admin-nav-group-system = Système

admin-nav-groups-title = Groupes
admin-nav-groups-description = Gérez les groupes d'utilisateurs et leurs membres
admin-nav-categories-title = Catégories
admin-nav-categories-description = Configurez les catégories de tickets et leur visibilité par groupe
admin-nav-assignment-rules-title = Règles d'affectation
admin-nav-assignment-rules-description = Configurez l'affectation automatique des tickets selon des règles
admin-nav-workflow-title = Workflow
admin-nav-workflow-description = Ajoutez des états de ticket nommés dans les catégories de workflow standard
admin-nav-sla-title = SLA
admin-nav-sla-description = Politiques de niveau de service et calendriers d'heures ouvrées
admin-nav-api-tokens-title = Jetons d'API
admin-nav-api-tokens-description = Gérez les jetons d'API pour l'accès programmatique
admin-nav-webhooks-title = Webhooks
admin-nav-webhooks-description = Configurez les webhooks pour envoyer des événements à des services externes
admin-nav-plugins-title = Plugins
admin-nav-plugins-description = Gérez les plugins installés et les intégrations
admin-nav-data-import-title = Import de données
admin-nav-data-import-description = Importez des données depuis Intune, des fichiers CSV et d'autres sources
admin-nav-channels-email-title = Réception d'e-mails
admin-nav-channels-email-description = Interrogez une boîte de support en IMAP et transformez les messages en tickets
admin-nav-email-queue-title = File d'e-mails
admin-nav-email-queue-description = File durable des e-mails sortants : statut, réessais, bounces et actions par ligne
admin-nav-email-suppressions-title = Suppressions d'e-mails
admin-nav-email-suppressions-description = Adresses bloquées pour l'envoi sortant, alimentées automatiquement par les bounces durs
admin-nav-audit-log-title = Journal d'audit
admin-nav-audit-log-description = Trace forensique des modifications, issue des triggers par table
admin-nav-branding-title = Marque
admin-nav-branding-description = Personnalisez l'apparence et la marque de l'application
admin-nav-email-settings-title = Configuration des e-mails
admin-nav-email-settings-description = Configurez les paramètres SMTP et envoyez des e-mails de test
admin-nav-guest-access-title = Accès invité
admin-nav-guest-access-description = Contrôlez ce que les visiteurs non authentifiés peuvent voir et soumettre
admin-nav-auth-providers-title = Fournisseurs d'authentification
admin-nav-auth-providers-description = Configurez le SSO, Microsoft Entra et l'authentification locale
admin-nav-search-title = Recherche
admin-nav-search-description = Gérez l'index de recherche et consultez les statistiques d'indexation
admin-nav-system-settings-title = Paramètres système
admin-nav-system-settings-description = Gérez le stockage, nettoyez les fichiers obsolètes et la maintenance
admin-nav-backup-restore-title = Sauvegarde et restauration
admin-nav-backup-restore-description = Exportez et restaurez les données système et les pièces jointes

# Admin : Paramètres système.
admin-system-title = Paramètres système
admin-system-storage-title = Gestion du stockage
admin-system-storage-description = Supprimez les anciennes images de profil et avatars qui ne sont plus nécessaires pour libérer de l'espace disque.
admin-system-storage-clean = Nettoyer
admin-system-storage-cleaning = Nettoyage...
admin-system-storage-confirm-title = Nettoyer les images obsolètes ?
admin-system-storage-confirm-message = Cette action est irréversible.
admin-system-storage-confirm-label = Nettoyer
admin-system-cleanup-success = Nettoyage terminé
admin-system-cleanup-failed = Échec du nettoyage
admin-system-cleanup-stat-avatars = Avatars :
admin-system-cleanup-stat-banners = Bannières :
admin-system-cleanup-stat-thumbnails = Miniatures :
admin-system-cleanup-stat-checked = Vérifiés :
admin-system-cleanup-stat-errors = Erreurs :
admin-system-cleanup-view-errors = Voir les erreurs ({ $count })
admin-system-cleanup-error-unexpected = Une erreur inattendue est survenue lors du nettoyage des images

# Admin : gestion de l'index de recherche.
admin-search-mgmt-title = Gestion de l'index de recherche
admin-search-mgmt-description = Gérez l'index plein texte pour les tickets, la documentation, les appareils et les utilisateurs.
admin-search-mgmt-stats-title = Statistiques de l'index
admin-search-mgmt-refresh = Actualiser
admin-search-mgmt-total-documents = Documents totaux
admin-search-mgmt-index-size = Taille de l'index
admin-search-mgmt-status = Statut
admin-search-mgmt-status-rebuilding = Reconstruction
admin-search-mgmt-status-ready = Prêt
admin-search-mgmt-entity-types = Types d'entités
admin-search-mgmt-stats-error = Échec de la récupération des statistiques de l'index
admin-search-mgmt-rebuild-title = Reconstruire l'index de recherche
admin-search-mgmt-rebuild-description = Reconstruit tout l'index depuis la base de données. Ré-indexe tous les tickets, commentaires, pages de documentation, pièces jointes, appareils et utilisateurs. Utilisez ceci si des résultats manquent ou sont obsolètes.
admin-search-mgmt-rebuild = Reconstruire l'index
admin-search-mgmt-rebuilding = Reconstruction...
admin-search-mgmt-rebuild-success = Index reconstruit avec succès
admin-search-mgmt-rebuild-failed = Échec de la reconstruction
admin-search-mgmt-rebuild-stat-tickets = Tickets :
admin-search-mgmt-rebuild-stat-comments = Commentaires :
admin-search-mgmt-rebuild-stat-docs = Documents :
admin-search-mgmt-rebuild-stat-attachments = Pièces jointes :
admin-search-mgmt-rebuild-stat-devices = Appareils :
admin-search-mgmt-rebuild-stat-users = Utilisateurs :
admin-search-mgmt-rebuild-stat-total = Total :
admin-search-mgmt-rebuild-confirm-title = Reconstruire l'index de recherche ?
admin-search-mgmt-rebuild-confirm-message = Cela peut prendre quelques instants selon le volume de données.
admin-search-mgmt-rebuild-confirm-label = Reconstruire
admin-search-mgmt-rebuild-error-unexpected = Une erreur inattendue est survenue lors de la reconstruction de l'index
