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
password-reset-page-title = Réinitialiser votre mot de passe
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

# Admin : Configuration des e-mails.
admin-email-settings-title = Configuration des e-mails
admin-email-settings-description = Consultez l'état de la configuration des e-mails et envoyez des e-mails de test. Les paramètres se configurent via des variables d'environnement.
admin-email-settings-env-notice-prefix = Les paramètres d'e-mail se configurent via des variables d'environnement dans votre fichier
admin-email-settings-env-notice-suffix = ou l'environnement Docker. Utilisez « Envoyer un e-mail de test » pour vérifier que votre configuration fonctionne.
admin-email-settings-loading = Chargement de la configuration des e-mails...
admin-email-settings-service = Service SMTP
admin-email-settings-configured = Configuré
admin-email-settings-not-configured = Non configuré
admin-email-settings-enabled = Activé
admin-email-settings-server = Serveur
admin-email-settings-username = Nom d'utilisateur
admin-email-settings-from-address = Adresse d'expéditeur
admin-email-settings-password = Mot de passe
admin-email-settings-password-not-set = Non défini
admin-email-settings-env-vars-label = Env :
admin-email-settings-test-send = Envoyer un test :
admin-email-settings-test-placeholder = destinataire@exemple.com
admin-email-settings-test-send-button = Envoyer
admin-email-settings-test-sending = Envoi...
admin-email-settings-empty-title = Les e-mails ne sont pas configurés
admin-email-settings-empty-description = Configurez les paramètres d'e-mail dans vos variables d'environnement pour activer la fonctionnalité
admin-email-settings-error-load = Échec du chargement de la configuration des e-mails
admin-email-settings-error-no-address = Veuillez saisir une adresse e-mail
admin-email-settings-error-bad-address = Veuillez saisir une adresse e-mail valide
admin-email-settings-test-success = E-mail de test envoyé
admin-email-settings-error-test = Échec de l'envoi de l'e-mail de test

# Admin : Accès invité.
admin-guest-title = Accès invité
admin-guest-description = Contrôlez ce que les visiteurs non authentifiés peuvent voir et soumettre. Toutes les fonctions sont désactivées par défaut.
admin-guest-loading = Chargement des paramètres invité...
admin-guest-features-title = Fonctions publiques
admin-guest-toggle-tickets-label = Accepter les tickets des invités
admin-guest-toggle-tickets-description = Affiche un formulaire public à /submit-ticket.
admin-guest-toggle-lookup-label = Suivi de ticket invité
admin-guest-toggle-lookup-description = Permet aux invités de vérifier l'état via un lien privé renvoyé à la soumission.
admin-guest-toggle-public-docs-label = Documentation publique
admin-guest-toggle-public-docs-description = Expose les pages marquées « public » à /docs sans authentification.
admin-guest-toggle-kb-search-label = Recherche dans la base de connaissances publique
admin-guest-toggle-kb-search-description = Recherche dans la documentation publique. Nécessite l'option « Documentation publique » activée.
admin-guest-toggle-help-label = Page d'aide en libre-service
admin-guest-toggle-help-description = Page statique /help avec des liens vers la réinitialisation et la soumission de ticket.
admin-guest-submissions-title = Soumissions de tickets invités
admin-guest-submissions-description = Comportement pour les tickets soumis via le formulaire public.
admin-guest-toggle-email-verification-label = Exiger la confirmation par e-mail
admin-guest-toggle-email-verification-description = Met les soumissions en attente jusqu'à confirmation. Donne aussi accès au portail.
admin-guest-toggle-attachments-label = Autoriser les pièces jointes
admin-guest-toggle-attachments-description = Les soumissions peuvent inclure des images, PDF et fichiers texte/log (≤10 Mo, max 5 par ticket).
admin-guest-default-priority-label = Priorité par défaut
admin-guest-default-priority-hint = Appliquée à toute soumission d'invité. Les techniciens peuvent re-trier ensuite.
admin-guest-priority-low = Basse
admin-guest-priority-medium = Moyenne
admin-guest-priority-high = Haute
admin-guest-intro-message-label = Message d'introduction
admin-guest-intro-message-optional = (facultatif)
admin-guest-intro-message-placeholder = ex. Pour les pannes urgentes, appelez le 555-1234. Consultez d'abord /docs.
admin-guest-intro-message-hint = Affiché au-dessus du formulaire public. Texte brut, les sauts de ligne sont préservés.
admin-guest-intro-message-count = { $count } / 500
admin-guest-rate-limit-label = Limite de débit
admin-guest-rate-limit-suffix = par IP / heure
admin-guest-rate-limit-hint = Baissez cette valeur si vous voyez du spam depuis des IP partagées.
admin-guest-unsaved = Modifications non enregistrées
admin-guest-save = Enregistrer les paramètres
admin-guest-saving = Enregistrement...
admin-guest-error-load = Échec du chargement des paramètres invité
admin-guest-error-save = Échec de l'enregistrement des paramètres invité
admin-guest-saved = Paramètres d'accès invité enregistrés

# Admin : Import de données.
admin-data-import-title = Import de données
admin-data-import-description = Importez et synchronisez des données depuis des sources externes
admin-data-import-notice = Les imports peuvent déclencher des notifications aux utilisateurs concernés. Les enregistrements existants sont mis à jour selon les ID correspondants.
admin-data-import-status-available = Disponible
admin-data-import-status-coming-soon = Bientôt disponible
admin-data-import-status-beta = Bêta
admin-data-import-microsoft-title = Microsoft Graph
admin-data-import-microsoft-description = Importez des données depuis Microsoft 365, y compris Azure AD, Intune et d'autres services Microsoft
admin-data-import-csv-title = Import CSV
admin-data-import-csv-description = Importez des données depuis des fichiers CSV (appareils, utilisateurs et autres ressources)
admin-data-import-api-title = Intégrations API
admin-data-import-api-description = Connectez-vous à des API tierces pour importer et synchroniser des données
admin-data-import-ad-title = Active Directory
admin-data-import-ad-description = Importez des données depuis des serveurs Active Directory locaux

# Admin : Fournisseurs d'authentification.
admin-auth-providers-title = Fournisseurs d'authentification
admin-auth-providers-env-notice-prefix = Les fournisseurs d'authentification se configurent via des variables d'environnement dans votre fichier
admin-auth-providers-env-notice-suffix = . Utilisez le bouton « Valider la config » pour vérifier la configuration de chaque fournisseur.
admin-auth-providers-loading = Chargement des fournisseurs...
admin-auth-providers-default-badge = Défaut
admin-auth-providers-configured = Configuré
admin-auth-providers-not-configured = Non configuré
admin-auth-providers-enabled = Activé
admin-auth-providers-client-id = ID client
admin-auth-providers-tenant-id = ID locataire
admin-auth-providers-redirect-uri = URI de redirection
admin-auth-providers-secret = Secret
admin-auth-providers-secret-not-set = Non défini
admin-auth-providers-env-label = Env :
admin-auth-providers-empty-title = Aucun fournisseur d'authentification trouvé
admin-auth-providers-empty-description = Configurez les fournisseurs d'authentification dans vos variables d'environnement
admin-auth-providers-error-load = Échec du chargement des fournisseurs d'authentification
admin-auth-providers-error-validate = Échec de la validation de la configuration

# Admin : Jetons d'API.
admin-api-tokens-title = Jetons d'API
admin-api-tokens-description = Gérez les jetons d'API pour l'accès programmatique
admin-api-tokens-create = Créer un jeton
admin-api-tokens-create-short = Créer
admin-api-tokens-loading = Chargement des jetons...
admin-api-tokens-active-heading = Jetons actifs
admin-api-tokens-revoked-heading = Jetons révoqués
admin-api-tokens-user-prefix = Utilisateur :
admin-api-tokens-created-prefix = Créé { $when }
admin-api-tokens-expires-prefix = Expire { $when }
admin-api-tokens-no-expiration = Sans expiration
admin-api-tokens-last-used-label = Dernière utilisation :
admin-api-tokens-last-used-never = Jamais
admin-api-tokens-revoked-prefix = Révoqué { $when }
admin-api-tokens-revoke-title = Révoquer le jeton
admin-api-tokens-error-load = Échec du chargement des jetons d'API
admin-api-tokens-error-create = Échec de la création du jeton
admin-api-tokens-error-revoke = Échec de la révocation du jeton
admin-api-tokens-error-name-required = Le nom du jeton est requis
admin-api-tokens-error-user-required = Veuillez sélectionner un utilisateur
admin-api-tokens-revoke-success = Jeton révoqué avec succès
admin-api-tokens-modal-create-title = Créer un jeton d'API
admin-api-tokens-modal-name-label = Nom du jeton
admin-api-tokens-modal-name-placeholder = ex. CI/CD Pipeline
admin-api-tokens-modal-name-hint = Un nom descriptif pour identifier ce jeton
admin-api-tokens-modal-user-label = Utilisateur (agit en tant que)
admin-api-tokens-modal-user-placeholder = Sélectionnez un utilisateur...
admin-api-tokens-modal-user-hint = Le jeton aura les mêmes permissions que cet utilisateur
admin-api-tokens-modal-expiration-label = Expiration
admin-api-tokens-modal-no-expiration-label = Sans expiration
admin-api-tokens-modal-expires-days-suffix = jours
admin-api-tokens-modal-expires-hint = Le jeton expirera après { $days } jours
admin-api-tokens-modal-no-expiration-warning = Les jetons sans expiration sont moins sécurisés
admin-api-tokens-modal-cancel = Annuler
admin-api-tokens-modal-creating = Création...
admin-api-tokens-created-title = Jeton créé
admin-api-tokens-created-warning = Copiez ce jeton maintenant, il ne sera plus affiché !
admin-api-tokens-copied = Copié !
admin-api-tokens-copy-title = Copier dans le presse-papiers
admin-api-tokens-bearer-hint-prefix = Utilisez ce jeton avec l'en-tête
admin-api-tokens-bearer-hint-suffix = .
admin-api-tokens-done = Terminé
admin-api-tokens-revoke-modal-title = Révoquer le jeton
admin-api-tokens-revoke-confirm-message = Confirmez-vous la révocation du jeton « { $name } » ?
admin-api-tokens-revoke-warning = Cette action est irréversible. Les systèmes utilisant ce jeton perdront l'accès.
admin-api-tokens-revoking = Révocation...

# Admin : SLA.
admin-sla-title = SLA
admin-sla-description = Les calendriers ouvrés et les politiques SLA alimentent la pastille SLA de chaque ticket.
admin-sla-loading = Chargement…
admin-sla-error-load = Échec du chargement de la configuration SLA
admin-sla-error-create = Échec de la création
admin-sla-error-delete = Échec de la suppression
admin-sla-error-update = Échec de la mise à jour
admin-sla-calendars-heading = Calendriers ouvrés
admin-sla-policies-heading = Politiques SLA
admin-sla-col-name = Nom
admin-sla-col-tz = Fuseau
admin-sla-col-default = Défaut
admin-sla-col-response = Réponse
admin-sla-col-resolution = Résolution
admin-sla-col-calendar = Calendrier
admin-sla-default-badge = Défaut
admin-sla-set-default = Définir par défaut
admin-sla-delete = Supprimer
admin-sla-calendar-delete-confirm = Supprimer ce calendrier ? Les politiques qui le référencent devront en choisir un autre.
admin-sla-policy-delete-confirm = Supprimer cette politique ? Les tickets qui en dépendent perdront leur pastille SLA jusqu'à ce qu'une autre politique s'applique. Action irréversible.
admin-sla-new-calendar-heading = Nouveau calendrier
admin-sla-new-policy-heading = Nouvelle politique
admin-sla-field-name = Nom
admin-sla-field-tz = Fuseau horaire
admin-sla-field-calendar = Calendrier
admin-sla-field-response = Réponse (minutes)
admin-sla-field-resolution = Résolution (minutes)
admin-sla-field-priority = Filtre de priorité
admin-sla-placeholder-name = Heures de support UE
admin-sla-placeholder-tz = Europe/Paris
admin-sla-policy-name-placeholder = Incidents critiques
admin-sla-schedule-hint = L'horaire par défaut est Lun-Ven 9h-17h. Modifiez à la main ou étendez ici plus tard.
admin-sla-priority-any = Toutes
admin-sla-priority-low = basse
admin-sla-priority-medium = moyenne
admin-sla-priority-high = haute
admin-sla-workspace-default = Défaut de l'espace de travail
admin-sla-create = Créer

# Admin : Marque.
admin-branding-title = Marque
admin-branding-description = Personnalisez l'apparence et la marque de l'application.
admin-branding-loading = Chargement de la configuration de marque...
admin-branding-general-heading = Paramètres généraux
admin-branding-app-name-label = Nom de l'application
admin-branding-app-name-placeholder = Nosdesk
admin-branding-app-name-hint = Ce nom apparaît dans l'en-tête et l'onglet du navigateur
admin-branding-primary-color-label = Couleur principale
admin-branding-primary-color-hint = Code couleur hexadécimal pour les éléments d'accent (ex. #2C80FF)
admin-branding-save = Enregistrer
admin-branding-saving = Enregistrement...
admin-branding-logo-heading = Logo
admin-branding-logo-dark-label = Logo thème sombre
admin-branding-logo-light-label = Logo thème clair (facultatif)
admin-branding-logo-upload = Téléverser le logo
admin-branding-logo-uploading = Téléversement...
admin-branding-logo-remove = Retirer
admin-branding-logo-formats = PNG, SVG, JPEG ou WebP. 2 Mo max.
admin-branding-logo-light-hint = Utilisé quand le thème clair est actif. Repli sur le logo principal.
admin-branding-favicon-heading = Favicon
admin-branding-favicon-upload = Téléverser le favicon
admin-branding-favicon-uploading = Téléversement...
admin-branding-favicon-formats = ICO, PNG ou SVG. Taille recommandée : 32x32 ou 64x64 pixels.
admin-branding-preview-heading = Aperçu
admin-branding-primary-color-preview = Couleur principale
admin-branding-configured = Marque personnalisée configurée
admin-branding-success-saved = Paramètres de marque enregistrés
admin-branding-success-logo = Logo téléversé
admin-branding-success-logo-light = Logo du thème clair téléversé
admin-branding-success-favicon = Favicon téléversé
admin-branding-success-removed = { $asset } retiré
admin-branding-error-load = Échec du chargement de la configuration de marque
admin-branding-error-save = Échec de l'enregistrement des paramètres de marque
admin-branding-error-invalid-file = Fichier invalide
admin-branding-error-upload-logo = Échec du téléversement du logo
admin-branding-error-upload-logo-light = Échec du téléversement du logo du thème clair
admin-branding-error-upload-favicon = Échec du téléversement du favicon
admin-branding-error-delete = Échec de la suppression de { $asset }
admin-branding-asset-logo = Logo
admin-branding-asset-logo-light = Logo du thème clair
admin-branding-asset-favicon = Favicon
admin-branding-confirm-title = Retirer { $asset } ?
admin-branding-confirm-message = Cela supprime l'image téléversée. Vous pouvez en remettre une, mais le fichier précédent ne sera pas récupérable.
admin-branding-confirm-remove = Retirer

# Admin : Sauvegarde et restauration.
admin-backup-title = Sauvegarde et restauration
admin-backup-description = Exportez et restaurez les données système et les pièces jointes
admin-backup-create-heading = Créer une sauvegarde
admin-backup-create-description = Exporter toutes les données système et pièces jointes dans une archive ZIP
admin-backup-include-sensitive-label = Inclure les données sensibles
admin-backup-include-sensitive-description = Inclut mots de passe, secrets MFA et jetons d'authentification (chiffrés avec un mot de passe)
admin-backup-encryption-warning = Les données sensibles seront chiffrées. Si vous perdez le mot de passe, les données seront irrécupérables.
admin-backup-encryption-password-label = Mot de passe de chiffrement
admin-backup-encryption-password-placeholder = Saisissez le mot de passe de chiffrement
admin-backup-confirm-password-label = Confirmer le mot de passe
admin-backup-confirm-password-placeholder = Confirmez le mot de passe de chiffrement
admin-backup-passwords-no-match = Les mots de passe ne correspondent pas
admin-backup-create-button = Créer la sauvegarde
admin-backup-creating = Création de la sauvegarde...
admin-backup-recent-heading = Sauvegardes récentes
admin-backup-refresh = Actualiser
admin-backup-empty = Aucune sauvegarde pour l'instant. Créez votre première sauvegarde ci-dessus.
admin-backup-encrypted-badge = Chiffrée
admin-backup-creating-status = Création...
admin-backup-download-title = Télécharger
admin-backup-delete-title = Supprimer
admin-backup-docs-heading = Exporter la documentation en Markdown
admin-backup-docs-description = Exportez toutes les pages de documentation en fichiers markdown dans une archive ZIP
admin-backup-docs-export = Exporter en Markdown
admin-backup-docs-exporting = Export { $current }/{ $total }...
admin-backup-docs-preparing = Préparation...
admin-backup-docs-error = Échec de l'export de la documentation. Consultez la console pour plus de détails.
admin-backup-restore-heading = Restaurer depuis une sauvegarde
admin-backup-restore-description = Téléversez un fichier de sauvegarde pour restaurer les données système et les pièces jointes
admin-backup-restore-dnd = Glissez-déposez un fichier de sauvegarde ici, ou
admin-backup-restore-browse = parcourez pour sélectionner un fichier
admin-backup-details-heading = Détails de la sauvegarde
admin-backup-detail-created = Créée :
admin-backup-detail-version = Version :
admin-backup-detail-files = Fichiers :
admin-backup-detail-size = Taille :
admin-backup-detail-tables = Tables :
admin-backup-warnings-heading = Avertissements
admin-backup-decryption-password-label = Mot de passe de déchiffrement
admin-backup-decryption-password-placeholder = Saisissez le mot de passe de chiffrement de la sauvegarde
admin-backup-restore-warning = La restauration remplacera les fichiers existants. Action irréversible.
admin-backup-restore-button = Restaurer les fichiers
admin-backup-restoring = Restauration...
admin-backup-cancel = Annuler
admin-backup-restore-not-zip = Sélectionnez un fichier .zip
admin-backup-upload-error = Échec du téléversement du fichier de sauvegarde
admin-backup-restore-success = Restauration terminée : { $files } fichiers restaurés. { $message }
admin-backup-restore-error = Échec de la restauration. Consultez la console pour plus de détails.
admin-backup-delete-confirm-title = Supprimer cette sauvegarde ?
admin-backup-delete-confirm-message = Le fichier de sauvegarde sera supprimé définitivement.
admin-backup-delete-confirm-label = Supprimer

# Admin : Règles d'affectation.
admin-assignment-rules-title = Règles d'affectation
admin-assignment-rules-description = Configurez l'affectation automatique des tickets selon des règles
admin-assignment-rules-new = Nouvelle règle
admin-assignment-rules-info = Les règles sont évaluées par ordre de priorité (de haut en bas). La première qui correspond gagne. Les tickets déjà affectés ne sont pas réaffectés automatiquement.
admin-assignment-rules-loading = Chargement des règles...
admin-assignment-rules-active = Active
admin-assignment-rules-inactive = Inactive
admin-assignment-rules-target-none = Non configurée
admin-assignment-rules-trigger-both = Les deux déclencheurs
admin-assignment-rules-trigger-create = À la création
admin-assignment-rules-trigger-category = Au changement de catégorie
admin-assignment-rules-trigger-none = Aucun déclencheur
admin-assignment-rules-assigned-count = { $count } affectés
admin-assignment-rules-move-up = Monter (priorité plus élevée)
admin-assignment-rules-move-down = Descendre (priorité plus basse)
admin-assignment-rules-toggle-deactivate = Désactiver la règle
admin-assignment-rules-toggle-activate = Activer la règle
admin-assignment-rules-edit = Modifier la règle
admin-assignment-rules-delete = Supprimer la règle
admin-assignment-rules-create-action = Créer une règle
admin-assignment-rules-error-load = Échec du chargement des règles d'affectation
admin-assignment-rules-error-name = Le nom de la règle est requis
admin-assignment-rules-error-user = Veuillez sélectionner un utilisateur cible
admin-assignment-rules-error-group = Veuillez sélectionner un groupe cible
admin-assignment-rules-error-save = Échec de l'enregistrement de la règle
admin-assignment-rules-error-update = Échec de la mise à jour de la règle
admin-assignment-rules-error-delete = Échec de la suppression de la règle
admin-assignment-rules-error-reorder = Échec de la réorganisation des règles
admin-assignment-rules-success-create = Règle créée avec succès
admin-assignment-rules-success-update = Règle mise à jour avec succès
admin-assignment-rules-success-delete = Règle supprimée avec succès
admin-assignment-rules-method-direct-label = Utilisateur direct
admin-assignment-rules-method-direct-description = Affecter directement à un utilisateur spécifique
admin-assignment-rules-method-round-robin-label = Round-Robin (groupe)
admin-assignment-rules-method-round-robin-description = Faire tourner l'affectation entre les membres du groupe de manière équitable
admin-assignment-rules-method-random-label = Aléatoire (groupe)
admin-assignment-rules-method-random-description = Sélectionner aléatoirement un membre du groupe pour chaque ticket
admin-assignment-rules-method-queue-label = File du groupe
admin-assignment-rules-method-queue-description = Affecter à la file du groupe (les utilisateurs récupèrent les tickets)
admin-assignment-rules-modal-create-title = Créer une règle d'affectation
admin-assignment-rules-modal-edit-title = Modifier la règle d'affectation
admin-assignment-rules-modal-name-label = Nom de la règle
admin-assignment-rules-modal-name-placeholder = ex. Round-Robin Support IT
admin-assignment-rules-modal-description-label = Description (facultatif)
admin-assignment-rules-modal-description-placeholder = Décrivez ce que fait cette règle...
admin-assignment-rules-modal-method-label = Méthode d'affectation
admin-assignment-rules-modal-user-label = Utilisateur cible
admin-assignment-rules-modal-user-placeholder = Sélectionnez un utilisateur...
admin-assignment-rules-modal-group-label = Groupe cible
admin-assignment-rules-modal-group-placeholder = Sélectionnez un groupe...
admin-assignment-rules-modal-group-members = { $count } membres
admin-assignment-rules-modal-category-label = Filtre de catégorie (facultatif)
admin-assignment-rules-modal-category-all = Toutes les catégories
admin-assignment-rules-modal-category-hint = N'affecter que les tickets de cette catégorie (vide = toutes)
admin-assignment-rules-modal-triggers-label = Déclencheurs
admin-assignment-rules-modal-trigger-create-label = À la création d'un ticket
admin-assignment-rules-modal-trigger-category-label = Au changement de catégorie d'un ticket
admin-assignment-rules-modal-active-label = Règle active
admin-assignment-rules-modal-cancel = Annuler
admin-assignment-rules-modal-saving = Enregistrement...
admin-assignment-rules-modal-update = Mettre à jour
admin-assignment-rules-modal-create = Créer la règle
admin-assignment-rules-delete-title = Supprimer la règle d'affectation
admin-assignment-rules-delete-message = Confirmez-vous la suppression de la règle « { $name } » ? Cette action est irréversible.
admin-assignment-rules-delete-cancel = Annuler
admin-assignment-rules-delete-confirm = Supprimer
admin-assignment-rules-deleting = Suppression...

# Admin: Categories (CategoriesManagementView).
admin-categories-title = Catégories
admin-categories-description = Gérer les catégories de tickets et la visibilité par groupe
admin-categories-new = Nouvelle catégorie
admin-categories-info = Les catégories sans restriction de groupe sont visibles par tous les utilisateurs. Associez des groupes pour restreindre la visibilité.
admin-categories-loading = Chargement des catégories...
admin-categories-search-placeholder = Rechercher des catégories...
admin-categories-filter-all = Toutes les catégories
admin-categories-filter-active = Actives uniquement
admin-categories-filter-inactive = Inactives uniquement
admin-categories-filter-public = Publiques uniquement
admin-categories-filter-restricted = Restreintes uniquement
admin-categories-sort-custom = Ordre personnalisé
admin-categories-sort-name = Nom
admin-categories-sort-ascending = Croissant
admin-categories-sort-descending = Décroissant
admin-categories-drag-handle = Glisser pour réorganiser
admin-categories-badge-public = Publique
admin-categories-badge-groups = { $count ->
    [one] { $count } groupe
   *[other] { $count } groupes
    }
admin-categories-badge-inactive = Inactive
admin-categories-groups-more = +{ $count } de plus
admin-categories-action-deactivate = Désactiver
admin-categories-action-activate = Activer
admin-categories-action-edit = Modifier la catégorie
admin-categories-action-delete = Supprimer la catégorie
admin-categories-no-search-results = Aucune catégorie correspondant à « { $query } »
admin-categories-no-filter-results = Aucune catégorie ne correspond au filtre actuel
admin-categories-empty-action = Créer une catégorie
admin-categories-modal-create-title = Créer une catégorie
admin-categories-modal-edit-title = Modifier la catégorie
admin-categories-modal-name-label = Nom
admin-categories-modal-name-placeholder = Saisir le nom de la catégorie
admin-categories-modal-description-label = Description
admin-categories-modal-description-placeholder = Description facultative
admin-categories-modal-icon-label = Icône
admin-categories-modal-color-label = Couleur
admin-categories-modal-active-label = Active
admin-categories-modal-visibility-label = Visible aux groupes
admin-categories-modal-visibility-hint = (laisser vide pour public)
admin-categories-modal-visibility-toggle-aria = Basculer la visibilité pour { $name }
admin-categories-modal-group-members = { $count } membres
admin-categories-modal-no-groups = Aucun groupe disponible.
admin-categories-modal-create-groups-link = Créer des groupes
admin-categories-modal-create-groups-suffix = d'abord.
admin-categories-modal-cancel = Annuler
admin-categories-modal-save = Enregistrer
admin-categories-modal-create = Créer la catégorie
admin-categories-delete-title = Supprimer la catégorie
admin-categories-delete-message = Confirmez-vous la suppression de la catégorie « { $name } » ? Les tickets utilisant cette catégorie verront leur catégorie effacée.
admin-categories-delete-cancel = Annuler
admin-categories-delete-confirm = Supprimer la catégorie
admin-categories-error-name-required = Le nom de la catégorie est obligatoire
admin-categories-error-load = Échec du chargement des catégories
admin-categories-error-reorder = Échec de la réorganisation des catégories
admin-categories-error-save = Échec de l'enregistrement de la catégorie
admin-categories-error-update = Échec de la mise à jour de la catégorie
admin-categories-error-delete = Échec de la suppression de la catégorie
admin-categories-success-create = Catégorie créée avec succès
admin-categories-success-update = Catégorie mise à jour avec succès
admin-categories-success-delete = Catégorie supprimée avec succès

# Admin: Email channels (ChannelsEmailSettingsView).
admin-channels-email-title = Ingestion des e-mails
admin-channels-email-description = Interrogez une boîte de support en IMAP et transformez les messages entrants en tickets. Les réponses des techniciens sont relayées dans le même fil.
admin-channels-email-loading = Chargement du canal...
admin-channels-email-status-heading = Statut
admin-channels-email-status-subtitle = Vue en direct de la dernière action du worker d'ingestion.
admin-channels-email-status-enabled = Activé
admin-channels-email-status-disabled = Désactivé
admin-channels-email-status-last-polled = Dernière interrogation
admin-channels-email-status-never = jamais
admin-channels-email-status-last-uid = Dernier UID vu
admin-channels-email-status-uid-validity = UIDVALIDITY
admin-channels-email-status-last-error = Dernière erreur
admin-channels-email-status-last-error-hint = Le worker continuera à réessayer avec un délai exponentiel. Corrigez le problème sous-jacent et il disparaîtra au prochain sondage réussi.
admin-channels-email-form-heading-edit = Configuration
admin-channels-email-form-heading-create = Connecter une boîte aux lettres
admin-channels-email-form-subtitle = IMAP sur TLS uniquement. Pour les serveurs de test auto-hébergés avec un certificat auto-signé, consultez l'option avancée ci-dessous.
admin-channels-email-toggle-enabled-label = Activé
admin-channels-email-toggle-enabled-description = Désactivé, le worker arrête de sonder mais la configuration et les identifiants stockés sont préservés.
admin-channels-email-field-name-label = Nom d'affichage
admin-channels-email-field-name-placeholder = ex. Boîte support
admin-channels-email-field-name-hint = Visible uniquement dans l'interface d'administration. Les clients ne le voient jamais.
admin-channels-email-field-host-label = Hôte IMAP
admin-channels-email-field-host-placeholder = imap.example.com
admin-channels-email-field-port-label = Port
admin-channels-email-field-port-hint = 993 pour IMAPS. 143 nécessite STARTTLS (pas encore pris en charge).
admin-channels-email-field-username-label = Nom d'utilisateur
admin-channels-email-field-username-placeholder = support@example.com
admin-channels-email-field-mailbox-label = Boîte aux lettres
admin-channels-email-field-mailbox-placeholder = INBOX
admin-channels-email-field-mailbox-hint = Les utilisateurs Gmail voudront peut-être « [Gmail]/All Mail ».
admin-channels-email-field-reply-domain-label = Domaine de réponse
admin-channels-email-field-reply-domain-placeholder = example.com
admin-channels-email-field-reply-domain-hint = Utilisé pour estampiller les Message-ID des réponses sortantes afin que la réponse du client revienne dans le même ticket. Généralement le même domaine que le nom d'utilisateur.
admin-channels-email-field-password-label = Mot de passe
admin-channels-email-field-password-keep-existing = (laisser vide pour conserver l'existant)
admin-channels-email-field-password-placeholder-stored = •••••••••• (enregistré)
admin-channels-email-field-password-placeholder-new = Mot de passe d'application ou de compte
admin-channels-email-remove-password = Supprimer le mot de passe enregistré
admin-channels-email-removing-password = Suppression...
admin-channels-email-advanced = Avancé
admin-channels-email-toggle-insecure-label = Ignorer la vérification du certificat TLS
admin-channels-email-toggle-insecure-description = UNIQUEMENT pour Greenmail ou des serveurs de test auto-hébergés avec un certificat auto-signé. À laisser désactivé en production.
admin-channels-email-test = Tester la connexion
admin-channels-email-testing = Test en cours...
admin-channels-email-test-connected = Connecté
admin-channels-email-test-failed = Échec
admin-channels-email-test-unknown-error = Erreur inconnue
admin-channels-email-delete = Supprimer
admin-channels-email-deleting = Suppression...
admin-channels-email-save = Enregistrer les modifications
admin-channels-email-saving = Enregistrement...
admin-channels-email-create = Créer le canal
admin-channels-email-creating = Création...
admin-channels-email-clear-credential-title = Supprimer le mot de passe enregistré ?
admin-channels-email-clear-credential-message = Le worker cessera de s'authentifier jusqu'à l'enregistrement d'un nouveau mot de passe.
admin-channels-email-clear-credential-confirm = Supprimer
admin-channels-email-delete-title = Supprimer ce canal e-mail ?
admin-channels-email-delete-message = Les tickets déjà créés à partir de ce canal restent intacts, mais aucun nouveau message ne sera ingéré. Cette action est irréversible.
admin-channels-email-delete-confirm = Supprimer le canal
admin-channels-email-relative-seconds = il y a { $count } s
admin-channels-email-relative-minutes = il y a { $count } min
admin-channels-email-relative-hours = il y a { $count } h
admin-channels-email-relative-days = il y a { $count } j
admin-channels-email-error-load = Échec du chargement du canal e-mail
admin-channels-email-success-update = Canal mis à jour
admin-channels-email-success-create = Canal créé
admin-channels-email-success-password-removed = Mot de passe supprimé
admin-channels-email-success-delete = Canal supprimé

# Admin : Microsoft Graph (import de données)
admin-msgraph-back = Retour à l'import de données
admin-msgraph-title = Microsoft Graph
admin-msgraph-subtitle = Gérer la synchronisation des données depuis les services Microsoft 365
admin-msgraph-sync-action = Synchroniser les données
admin-msgraph-syncing = Synchronisation...
admin-msgraph-api-name = API Microsoft Graph
admin-msgraph-status-connected = Connecté
admin-msgraph-status-disconnected = Non connecté
admin-msgraph-status-connecting = Connexion...
admin-msgraph-status-error = Erreur
admin-msgraph-config-valid = Configuré
admin-msgraph-config-invalid = Non configuré
admin-msgraph-field-client-id = Client ID
admin-msgraph-field-tenant-id = Tenant ID
admin-msgraph-field-secret = Secret
admin-msgraph-field-not-set = Non défini
admin-msgraph-secret-configured = Configuré
admin-msgraph-secret-not-set = Non défini
admin-msgraph-last-synced = Dernière synchronisation :
admin-msgraph-missing-config = Configuration requise manquante :
admin-msgraph-env-label = Env :
admin-msgraph-progress-title = Synchronisation en cours
admin-msgraph-progress-step = Étape { $current } sur { $total }
admin-msgraph-progress-status-running = en cours
admin-msgraph-progress-status-starting = démarrage
admin-msgraph-progress-status-completed = terminée
admin-msgraph-progress-status-completed-with-errors = Terminée avec des erreurs
admin-msgraph-progress-status-cancelling = annulation
admin-msgraph-progress-status-cancelled = annulée
admin-msgraph-progress-status-error = erreur
admin-msgraph-cancel = Annuler
admin-msgraph-monitor = Suivre
admin-msgraph-delta-badge = Delta
admin-msgraph-last-sync-title = Dernière synchronisation
admin-msgraph-last-sync-status-completed = Terminée
admin-msgraph-last-sync-status-completed-with-errors = Terminée avec des erreurs
admin-msgraph-last-sync-status-error = Erreur
admin-msgraph-last-sync-status-cancelled = Annulée
admin-msgraph-last-sync-type = Type
admin-msgraph-last-sync-type-delta = Delta
admin-msgraph-last-sync-type-full = Complète
admin-msgraph-last-sync-started = Démarrée
admin-msgraph-last-sync-duration = Durée
admin-msgraph-last-sync-items-processed = Éléments traités
admin-msgraph-last-sync-cancelled-value = Annulée
admin-msgraph-last-sync-failed-value = Échec
admin-msgraph-modal-title = Synchroniser les données depuis Microsoft Graph
admin-msgraph-modal-description = Sélectionnez les données à importer depuis Microsoft Graph :
admin-msgraph-entity-users-name = Utilisateurs
admin-msgraph-entity-users-description = Importer les comptes utilisateurs et profils depuis Microsoft Entra ID
admin-msgraph-entity-devices-name = Appareils
admin-msgraph-entity-devices-description = Importer les appareils gérés depuis Microsoft Intune avec les affectations utilisateurs
admin-msgraph-entity-groups-name = Groupes
admin-msgraph-entity-groups-description = Importer les groupes de sécurité et de distribution depuis Microsoft Entra ID
admin-msgraph-modal-info = La synchronisation importe les données les plus récentes depuis les services Microsoft. Cela peut prendre plusieurs minutes selon le volume.
admin-msgraph-results-title = Résultats de synchronisation
admin-msgraph-results-items = { $processed } / { $total } éléments
admin-msgraph-results-percent = ({ $percent } %)
admin-msgraph-results-more-errors = ... et { $count } erreurs supplémentaires
admin-msgraph-results-total-processed = Total traité :
admin-msgraph-results-total-processed-value = { $count } éléments
admin-msgraph-results-total-errors = Total des erreurs :
admin-msgraph-full-sync = Synchronisation complète
admin-msgraph-start-sync = Démarrer la synchronisation
admin-msgraph-starting = Démarrage...
admin-msgraph-sync-type-users = Comptes utilisateurs
admin-msgraph-sync-type-profile-photos = Photos de profil
admin-msgraph-sync-type-devices = Appareils gérés
admin-msgraph-sync-type-groups = Groupes de sécurité
admin-msgraph-time-just-now = À l'instant
admin-msgraph-time-minutes = il y a { $count } min
admin-msgraph-time-hours = il y a { $count } h
admin-msgraph-time-days = il y a { $count } j
admin-msgraph-duration-seconds = { $seconds } s
admin-msgraph-duration-minutes = { $minutes } min { $seconds } s
admin-msgraph-duration-hours = { $hours } h { $minutes } min
admin-msgraph-error-validate-config = Échec de la validation de la configuration
admin-msgraph-error-fetch-status = Échec de la récupération du statut de connexion
admin-msgraph-error-start-sync = Échec du démarrage de la synchronisation
admin-msgraph-error-cancel-sync = Échec de l'annulation de la synchronisation
admin-msgraph-success-sync-started = Synchronisation démarrée
admin-msgraph-success-cancel-requested = Annulation de la synchronisation demandée

# Admin: Registre des plugins (parcourir et installer)
admin-plugins-registry-back = Plugins installés
admin-plugins-registry-title = Registre des plugins
admin-plugins-registry-subtitle-before = Parcourez et installez les plugins publiés sur
admin-plugins-registry-subtitle-after = . Les signatures sont vérifiées avec la clé racine Nosdesk avant l'exécution de tout paquet.
admin-plugins-registry-refresh = Actualiser
admin-plugins-registry-refreshing = Actualisation
admin-plugins-registry-loading = Chargement du registre...
admin-plugins-registry-disabled-title = La synchronisation du registre est désactivée
admin-plugins-registry-disabled-description-sideload = Cette instance a NOSDESK_REGISTRY_URL défini sur vide, elle ne récupère donc pas le catalogue de plugins publiés. Vous pouvez toujours installer manuellement un zip signé.
admin-plugins-registry-disabled-description-cli = Cette instance a NOSDESK_REGISTRY_URL défini sur vide, elle ne récupère donc pas le catalogue de plugins publiés. Utilisez la CLI pour installer des plugins signés localement.
admin-plugins-registry-disabled-action = Installer un zip signé
admin-plugins-registry-pending-title = Synchronisation du registre en cours
admin-plugins-registry-pending-description = L'instance récupère le catalogue de plugins publiés. Cela se termine généralement quelques secondes après le démarrage.
admin-plugins-registry-failed-title = Échec de la synchronisation du registre
admin-plugins-registry-failed-description = { $reason }. Réessayez maintenant pour récupérer à nouveau, ou attendez la prochaine tentative planifiée.
admin-plugins-registry-retry-now = Réessayer maintenant
admin-plugins-registry-search-label = Rechercher des plugins
admin-plugins-registry-search-placeholder = Rechercher des plugins
admin-plugins-registry-filter-aria = Filtrer le registre
admin-plugins-registry-trust-tier = Niveau de confiance
admin-plugins-registry-tier-official = Officiel
admin-plugins-registry-tier-verified = Vérifié
admin-plugins-registry-tier-community = Communauté
admin-plugins-registry-tier-local = Local
admin-plugins-registry-reset-filters = Réinitialiser les filtres
admin-plugins-registry-snapshot-fetched = Instantané récupéré { $relative }
admin-plugins-registry-result-count = { $filtered } sur { $total } { $total ->
    [one] plugin
   *[other] plugins
   }
admin-plugins-registry-no-matches = Aucun plugin ne correspond à ces filtres.
admin-plugins-registry-installed-badge = Installé
admin-plugins-registry-manage = Gérer
admin-plugins-registry-install = Installer
admin-plugins-registry-installing = Installation...
admin-plugins-registry-sr-plugin-name = Nom du plugin
admin-plugins-registry-sr-publisher = Éditeur
admin-plugins-registry-sr-homepage = Site web
admin-plugins-registry-by-publisher = par { $publisher }
admin-plugins-registry-homepage-link = Site web
admin-plugins-registry-publisher-nosdesk = Nosdesk
admin-plugins-registry-publisher-unknown = Éditeur inconnu
admin-plugins-registry-modal-title = Installer { $name } ?
admin-plugins-registry-community-warning-strong = Plugin communautaire.
admin-plugins-registry-community-warning-body = Nosdesk ne garantit pas la sécurité des plugins communautaires au-delà de la vérification de la signature de l'éditeur. Examinez le code source avant de lui confier vos données.
admin-plugins-registry-field-publisher = Éditeur
admin-plugins-registry-field-fingerprint = Empreinte
admin-plugins-registry-field-version = Version
admin-plugins-registry-type-to-confirm-before = Saisissez
admin-plugins-registry-type-to-confirm-after = pour confirmer
admin-plugins-registry-cancel = Annuler
admin-plugins-registry-error-load = Échec du chargement du registre.
admin-plugins-registry-error-refresh = Échec de la nouvelle tentative de synchronisation du registre.
admin-plugins-registry-error-confirm-name = Saisissez exactement le nom du plugin pour confirmer l'installation.
admin-plugins-registry-error-install = Échec de l'installation.
admin-plugins-registry-success-installed = { $name } v{ $version } installé
admin-plugins-registry-relative-just-now = à l'instant
admin-plugins-registry-relative-minutes = il y a { $count } min
admin-plugins-registry-relative-hours = il y a { $count } h
admin-plugins-registry-relative-days = { $count ->
    [one] il y a { $count } jour
   *[other] il y a { $count } jours
   }

# Admin: Webhooks (gestion des envois d'événements sortants)
admin-webhooks-title = Webhooks
admin-webhooks-subtitle = Gérez les webhooks pour les intégrations externes
admin-webhooks-create = Créer un webhook
admin-webhooks-create-short = Créer
admin-webhooks-loading = Chargement des webhooks...
admin-webhooks-section-active = Webhooks actifs
admin-webhooks-section-disabled = Webhooks désactivés
admin-webhooks-status-active = Actif
admin-webhooks-status-warning = Avertissement
admin-webhooks-status-failing = En échec
admin-webhooks-status-disabled = Désactivé
admin-webhooks-failure-count = { $count ->
    [one] { $count } échec
   *[other] { $count } échecs
   }
admin-webhooks-meta-secret = Secret :
admin-webhooks-meta-events = { $count ->
    [one] { $count } événement
   *[other] { $count } événements
   }
admin-webhooks-meta-last-triggered = Dernier déclenchement : { $when }
admin-webhooks-meta-never = Jamais
admin-webhooks-action-send-test = Envoyer un événement de test
admin-webhooks-action-view-deliveries = Voir les livraisons
admin-webhooks-action-edit = Modifier le webhook
admin-webhooks-action-delete = Supprimer le webhook
admin-webhooks-modal-create-title = Créer un webhook
admin-webhooks-modal-edit-title = Modifier le webhook
admin-webhooks-modal-secret-title = Webhook créé
admin-webhooks-modal-regenerate-title = Régénérer le secret
admin-webhooks-modal-delete-title = Supprimer le webhook
admin-webhooks-modal-deliveries-title = Historique des livraisons - { $name }
admin-webhooks-form-name-label = Nom
admin-webhooks-form-name-placeholder = par ex. Notifications Slack
admin-webhooks-form-url-label = URL de la charge utile
admin-webhooks-form-url-placeholder = https://example.com/webhook
admin-webhooks-form-url-hint = Les requêtes POST seront envoyées à cette URL
admin-webhooks-form-events-label = Événements
admin-webhooks-form-events-hint = Sélectionnez les événements qui déclenchent ce webhook
admin-webhooks-form-events-count = { $selected }/{ $total }
admin-webhooks-form-headers-label = En-têtes personnalisés
admin-webhooks-form-headers-add = + Ajouter un en-tête
admin-webhooks-form-headers-name-placeholder = Nom de l'en-tête
admin-webhooks-form-headers-value-placeholder = Valeur
admin-webhooks-form-headers-empty = Aucun en-tête personnalisé
admin-webhooks-form-enabled-label = Activé
admin-webhooks-form-enabled-description = Le webhook reçoit les événements lorsqu'il est activé
admin-webhooks-form-secret-label = Secret
admin-webhooks-form-secret-regenerate = Régénérer
admin-webhooks-form-cancel = Annuler
admin-webhooks-form-create = Créer le webhook
admin-webhooks-form-creating = Création...
admin-webhooks-form-save = Enregistrer les modifications
admin-webhooks-form-saving = Enregistrement...
admin-webhooks-secret-warning = Copiez ce secret maintenant, il ne sera plus affiché !
admin-webhooks-secret-helper-before = Utilisez ce secret pour vérifier les signatures des webhooks via l'en-tête
admin-webhooks-secret-helper-after = { "" }
admin-webhooks-secret-copy = Copier dans le presse-papiers
admin-webhooks-secret-copied = Copié !
admin-webhooks-secret-done = Terminé
admin-webhooks-regenerate-question = Voulez-vous vraiment régénérer le secret de { $name } ?
admin-webhooks-regenerate-warning = Le secret actuel sera invalidé. Vous devrez mettre à jour votre intégration avec le nouveau secret.
admin-webhooks-regenerate-confirm = Régénérer
admin-webhooks-regenerate-running = Régénération...
admin-webhooks-delete-question = Voulez-vous vraiment supprimer le webhook { $name } ?
admin-webhooks-delete-warning = Cette action est irréversible. Tout l'historique des livraisons sera perdu.
admin-webhooks-delete-confirm = Supprimer le webhook
admin-webhooks-delete-running = Suppression...
admin-webhooks-deliveries-loading = Chargement des livraisons...
admin-webhooks-deliveries-empty-title = Aucune livraison pour l'instant
admin-webhooks-deliveries-empty-description = Les livraisons apparaîtront ici une fois les événements déclenchés
admin-webhooks-deliveries-status-error = Erreur
admin-webhooks-deliveries-status-pending = En attente
admin-webhooks-deliveries-attempt = Tentative { $number }
admin-webhooks-deliveries-duration = { $ms } ms
admin-webhooks-deliveries-close = Fermer
admin-webhooks-error-name-required = Le nom du webhook est obligatoire
admin-webhooks-error-url-required = L'URL est obligatoire
admin-webhooks-error-event-required = Sélectionnez au moins un événement
admin-webhooks-error-load = Échec du chargement des webhooks
admin-webhooks-error-create = Échec de la création du webhook
admin-webhooks-error-update = Échec de la mise à jour du webhook
admin-webhooks-error-delete = Échec de la suppression du webhook
admin-webhooks-error-test = Échec de l'envoi de l'événement de test
admin-webhooks-error-regenerate = Échec de la régénération du secret
admin-webhooks-success-update = Webhook mis à jour
admin-webhooks-success-delete = Webhook supprimé
admin-webhooks-success-test = Événement de test envoyé au webhook
admin-webhooks-success-regenerate = Secret régénéré, consultez les livraisons du webhook pour la nouvelle signature
admin-webhooks-category-tickets = Tickets
admin-webhooks-category-comments = Commentaires
admin-webhooks-category-attachments = Pièces jointes
admin-webhooks-category-devices = Appareils
admin-webhooks-category-projects = Projets
admin-webhooks-category-documentation = Documentation
admin-webhooks-category-users = Utilisateurs
admin-webhooks-event-ticket-created = Ticket créé
admin-webhooks-event-ticket-updated = Ticket mis à jour
admin-webhooks-event-ticket-deleted = Ticket supprimé
admin-webhooks-event-ticket-linked = Ticket lié
admin-webhooks-event-ticket-unlinked = Ticket délié
admin-webhooks-event-comment-added = Commentaire ajouté
admin-webhooks-event-comment-deleted = Commentaire supprimé
admin-webhooks-event-attachment-added = Pièce jointe ajoutée
admin-webhooks-event-attachment-deleted = Pièce jointe supprimée
admin-webhooks-event-device-linked = Appareil lié
admin-webhooks-event-device-unlinked = Appareil délié
admin-webhooks-event-device-updated = Appareil mis à jour
admin-webhooks-event-project-assigned = Projet attribué
admin-webhooks-event-project-unassigned = Projet retiré
admin-webhooks-event-documentation-updated = Documentation mise à jour
admin-webhooks-event-user-created = Utilisateur créé
admin-webhooks-event-user-updated = Utilisateur mis à jour
admin-webhooks-event-user-deleted = Utilisateur supprimé

# Users list (UsersListView): people directory with role filter,
# bulk role change, and bulk delete.
user-mgmt-search-placeholder = Rechercher des utilisateurs...
user-mgmt-item-label = utilisateur
user-mgmt-filter-all-roles = Tous les rôles
user-mgmt-role-admin = Administrateur
user-mgmt-role-technician = Technicien
user-mgmt-role-user = Utilisateur
user-mgmt-column-user = Utilisateur
user-mgmt-column-role = Rôle
user-mgmt-column-tickets = Tickets
user-mgmt-column-devices = Appareils
user-mgmt-column-joined = Inscrit le
user-mgmt-invite-action = Inviter un utilisateur
user-mgmt-mobile-tickets = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
   }
user-mgmt-mobile-devices = { $count ->
    [one] { $count } appareil
   *[other] { $count } appareils
   }
user-mgmt-bulk-role = Rôle
user-mgmt-bulk-delete = Supprimer
user-mgmt-bulk-delete-count = Supprimer { $count }
user-mgmt-bulk-delete-title = { $count ->
    [one] Supprimer l'utilisateur ?
   *[other] Supprimer { $count } utilisateurs ?
}
user-mgmt-bulk-delete-message = { $count ->
    [one] Cela supprimera définitivement un utilisateur. Cette action est irréversible.
   *[other] Cela supprimera définitivement { $count } utilisateurs. Cette action est irréversible.
}
user-mgmt-bulk-action-error = Échec de l'action groupée. Veuillez réessayer.
user-mgmt-role-modal-title = Définir le rôle
user-mgmt-role-modal-body = { $count ->
    [one] Modifier le rôle de { $count } utilisateur
   *[other] Modifier le rôle de { $count } utilisateurs
   }

# Profil utilisateur (UserProfileView) : détail d'un utilisateur,
# formulaire de création, panneaux appareils/groupes et tickets.
user-profile-document-title = Profil de { $name } | Nosdesk
user-profile-back-to-users = Retour aux utilisateurs
user-profile-action-profile-settings = Paramètres du profil
user-profile-action-user-settings = Paramètres utilisateur
user-profile-create-title = Créer un nouvel utilisateur
user-profile-create-subtitle = Ajoutez un nouvel utilisateur à votre organisation
user-profile-section-basic-info = Informations de base
user-profile-field-name = Nom complet
user-profile-field-name-placeholder = Saisissez le nom complet
user-profile-field-email = Adresse e-mail
user-profile-field-email-placeholder = utilisateur@exemple.com
user-profile-field-role = Rôle
user-profile-field-role-placeholder = Sélectionnez un rôle
user-profile-role-user = Utilisateur
user-profile-role-technician = Technicien
user-profile-role-admin = Administrateur
user-profile-field-pronouns = Pronoms
user-profile-field-pronouns-placeholder = ex. il/lui, elle/elle, iel
user-profile-section-account-setup = Configuration du compte
user-profile-smtp-warning-title = E-mail non configuré
user-profile-smtp-warning-body = Vous devez définir un mot de passe manuellement, les invitations par e-mail étant indisponibles.
user-profile-setup-method = Méthode de configuration
user-profile-setup-invite-title = Envoyer une invitation par e-mail
user-profile-setup-invite-body = L'utilisateur recevra un e-mail avec un lien sécurisé pour définir son mot de passe
user-profile-setup-password-title = Définir un mot de passe manuellement
user-profile-setup-password-body = Créez un mot de passe pour l'utilisateur maintenant et partagez-le en toute sécurité
user-profile-field-password = Mot de passe
user-profile-field-password-placeholder = 8 caractères minimum
user-profile-field-confirm-password = Confirmer le mot de passe
user-profile-field-confirm-password-placeholder = Saisissez à nouveau le mot de passe
user-profile-passwords-match = Les mots de passe correspondent
user-profile-passwords-no-match = Les mots de passe ne correspondent pas
user-profile-required-note = Champs obligatoires
user-profile-action-cancel = Annuler
user-profile-action-create = Créer l'utilisateur
user-profile-action-creating = Création...
user-profile-devices-title = Appareils
user-profile-devices-empty = Aucun appareil
user-profile-device-manufacturer-unknown = Inconnu
user-profile-device-last-updated = Dernière mise à jour { $when }
user-profile-groups-title = Groupes
user-profile-not-found = Utilisateur introuvable
user-profile-error-no-create-permission = Vous n'avez pas l'autorisation de créer des utilisateurs
user-profile-error-missing-id = L'identifiant de l'utilisateur est manquant
user-profile-error-password-too-short = Le mot de passe doit comporter au moins 8 caractères
user-profile-error-passwords-mismatch = Les mots de passe ne correspondent pas
user-profile-error-created-no-uuid = Utilisateur créé mais la navigation a échoué. Rendez-vous sur la liste des utilisateurs.
user-profile-error-save-generic = Échec de l'enregistrement de l'utilisateur. Veuillez réessayer.
user-profile-error-load = Échec du chargement du profil utilisateur
user-profile-relative-just-now = à l'instant
user-profile-relative-minutes-ago = { $count ->
    [one] il y a { $count } minute
   *[other] il y a { $count } minutes
   }
user-profile-relative-hours-ago = { $count ->
    [one] il y a { $count } heure
   *[other] il y a { $count } heures
   }
user-profile-relative-days-ago = { $count ->
    [one] il y a { $count } jour
   *[other] il y a { $count } jours
   }

# Groups management (GroupsManagementView): list, search/sort,
# create modal, delete confirm, and member/device/group count chips.
groups-mgmt-title = Groupes
groups-mgmt-subtitle = Gérer les groupes d'utilisateurs et leurs membres
groups-mgmt-action-new = Nouveau groupe
groups-mgmt-action-new-short = Nouveau
groups-mgmt-loading = Chargement des groupes...
groups-mgmt-search-placeholder = Rechercher des groupes...
groups-mgmt-sort-name = Nom
groups-mgmt-sort-members = Membres
groups-mgmt-sort-devices = Appareils
groups-mgmt-sort-created = Date d'ajout
groups-mgmt-sort-ascending = Croissant
groups-mgmt-sort-descending = Décroissant
groups-mgmt-chip-members = { $count ->
    [one] { $count } membre
   *[other] { $count } membres
   }
groups-mgmt-chip-devices = { $count ->
    [one] { $count } appareil
   *[other] { $count } appareils
   }
groups-mgmt-chip-groups = { $count ->
    [one] { $count } groupe
   *[other] { $count } groupes
   }
groups-mgmt-action-open-full-page = Ouvrir la page complète
groups-mgmt-action-delete = Supprimer le groupe
groups-mgmt-no-results = Aucun groupe correspondant à « { $query } »
groups-mgmt-empty-action = Créer un groupe
groups-mgmt-modal-create-title = Créer un groupe
groups-mgmt-field-name = Nom
groups-mgmt-field-name-placeholder = Saisir le nom du groupe
groups-mgmt-field-description = Description
groups-mgmt-field-description-placeholder = Description facultative
groups-mgmt-field-color = Couleur
groups-mgmt-action-cancel = Annuler
groups-mgmt-action-create = Créer le groupe
groups-mgmt-modal-delete-title = Supprimer le groupe
groups-mgmt-delete-confirm-body = Voulez-vous vraiment supprimer le groupe <strong class="text-primary">{ $name }</strong> ? Cette action supprimera toutes les associations de membres mais ne supprimera pas les utilisateurs.
groups-mgmt-action-delete-confirm = Supprimer le groupe
groups-mgmt-error-name-required = Le nom du groupe est obligatoire
groups-mgmt-error-load = Échec du chargement des groupes
groups-mgmt-error-create = Échec de la création du groupe
groups-mgmt-error-delete = Échec de la suppression du groupe
groups-mgmt-success-created = Groupe créé avec succès
groups-mgmt-success-deleted = Groupe supprimé avec succès

# Group detail (GroupDetailView): per-group page showing
# sync status, members, devices, and creation metadata.
group-detail-error-invalid-id = Identifiant de groupe invalide
group-detail-error-load = Échec du chargement des détails du groupe
group-detail-sync-source-microsoft = Microsoft Entra ID
group-detail-type-security = Sécurité
group-detail-type-mail-enabled = Messagerie activée
group-detail-type-standard = Standard
group-detail-synced-from = Synchronisé depuis { $source }
group-detail-action-configure = Configurer
group-detail-section-information = Informations sur le groupe
group-detail-field-type = Type
group-detail-field-sync-source = Source de synchronisation
group-detail-field-last-synced = Dernière synchronisation
group-detail-field-created = Créé
group-detail-field-updated = Mis à jour
group-detail-section-members = Membres
group-detail-section-devices = Appareils
group-detail-no-members = Aucun membre
group-detail-no-devices = Aucun appareil
group-detail-unknown-device = Appareil inconnu
group-detail-not-found = Groupe introuvable

# Devices list (DevicesListView): paginated table with warranty
# filter, sortable columns, bulk delete, and mobile row layout.
devices-list-search-placeholder = Rechercher des appareils...
devices-list-item-label = appareil
devices-list-filter-warranty-active = Active
devices-list-filter-warranty-warning = À surveiller
devices-list-filter-warranty-expired = Expirée
devices-list-filter-warranty-unknown = Inconnue
devices-list-filter-warranty-all = Toutes les garanties
devices-list-column-device = Appareil
devices-list-column-serial = Numéro de série
devices-list-column-hostname = Nom d'hôte
devices-list-column-model = Modèle
devices-list-column-user = Utilisateur
devices-list-column-warranty = Garantie
devices-list-add-action = Ajouter un appareil
devices-list-unassigned = Non attribué
devices-list-warranty-unknown = Inconnue
devices-list-bulk-delete = Supprimer
devices-list-bulk-delete-count = Supprimer { $count }
devices-list-bulk-delete-title = { $count ->
    [one] Supprimer l'appareil ?
   *[other] Supprimer { $count } appareils ?
}
devices-list-bulk-delete-message = { $count ->
    [one] Cela supprimera définitivement un appareil. Cette action est irréversible.
   *[other] Cela supprimera définitivement { $count } appareils. Cette action est irréversible.
}
devices-list-bulk-action-error = Échec de la suppression des appareils. Veuillez réessayer.

# Device detail (DeviceView): per-device page covering name, hostname,
# hardware identifiers, warranty fields, primary user, Microsoft Intune
# integration, and the unmanage / create flows.
device-detail-back-to-ticket = Retour au ticket n°{ $id }
device-detail-back-to-devices = Retour
device-detail-readonly = Lecture seule
device-detail-delete-item-name = Appareil
device-detail-error-invalid-id = ID d'appareil invalide
device-detail-error-load = Échec du chargement des détails de l'appareil
device-detail-error-create = Échec de la création de l'appareil. Veuillez réessayer.
device-detail-error-delete = Échec de la suppression de l'appareil. Veuillez réessayer.
device-detail-error-unmanage = Échec de la désinscription de l'appareil. Veuillez réessayer.
device-detail-section-details = Détails de l'appareil
device-detail-field-name = Nom
device-detail-field-name-placeholder-create = Saisir le nom de l'appareil
device-detail-field-name-placeholder-edit = Saisir un nom...
device-detail-field-hostname = Nom d'hôte
device-detail-field-hostname-placeholder-create = Saisir le nom d'hôte
device-detail-field-hostname-placeholder-edit = Saisir le nom d'hôte...
device-detail-field-serial = Numéro de série
device-detail-field-serial-placeholder-create = Saisir le numéro de série
device-detail-field-serial-placeholder-edit = Saisir le numéro de série...
device-detail-field-manufacturer = Fabricant
device-detail-field-manufacturer-placeholder-create = ex. Dell, HP, Apple
device-detail-field-manufacturer-placeholder-edit = Saisir le fabricant...
device-detail-field-model = Modèle
device-detail-field-model-placeholder-create = Saisir le modèle de l'appareil
device-detail-field-model-placeholder-edit = Saisir le modèle...
device-detail-field-warranty-status = Statut de garantie
device-detail-field-warranty-start = Début de garantie
device-detail-field-warranty-end = Fin de garantie
device-detail-field-purchase-date = Date d'achat
device-detail-field-asset-tag = Étiquette d'inventaire
device-detail-field-asset-tag-placeholder-create = Saisir l'étiquette d'inventaire
device-detail-field-asset-tag-placeholder-edit = Saisir l'étiquette d'inventaire...
device-detail-warranty-active = Active
device-detail-warranty-warning = Avertissement
device-detail-warranty-expired = Expirée
device-detail-warranty-unknown = Inconnue
device-detail-section-primary-user = Utilisateur principal
device-detail-no-user-assigned = Aucun utilisateur attribué à cet appareil
device-detail-action-assign-user = Attribuer un utilisateur
device-detail-action-change-user = Changer d'utilisateur
device-detail-section-device-information = Informations sur l'appareil
device-detail-field-device-id = ID de l'appareil
device-detail-field-created = Créé
device-detail-field-last-updated = Dernière mise à jour
device-detail-manually-managed = Géré manuellement
device-detail-manually-managed-description = Cet appareil a été créé et est géré manuellement dans Nosdesk
device-detail-section-microsoft-integration = Intégration Microsoft
device-detail-field-last-intune-check-in = Dernière connexion Intune
device-detail-action-view-in-intune = Voir dans Intune
device-detail-action-view-in-entra = Voir dans Entra
device-detail-action-unmanage = Désinscrire d'Intune/Entra
device-detail-action-unmanage-processing = Traitement en cours...
device-detail-action-unmanage-title = Retirer de la gestion Microsoft Intune/Entra
device-detail-unmanage-conversion-note = L'appareil passera en gestion manuelle
device-detail-tech-details-show = Afficher les détails techniques
device-detail-tech-details-hide = Masquer les détails techniques
device-detail-field-intune-id = ID Intune
device-detail-field-entra-id = ID Entra
device-detail-not-managed-by-intune = Cet appareil n'est pas géré par Microsoft Intune
device-detail-action-cancel = Annuler
device-detail-action-create = Créer l'appareil
device-detail-action-create-processing = Création en cours...
device-detail-not-found = Appareil introuvable
device-detail-unmanage-modal-title = Désinscrire l'appareil
device-detail-unmanage-heading = Désinscrire de Microsoft
device-detail-unmanage-confirm-body = Confirmez-vous la désinscription de <strong class="text-primary">{ $name }</strong> de Microsoft Intune/Entra ?
device-detail-unmanage-confirm-note = L'appareil passera en gestion manuelle. Vous pourrez modifier tous les champs, mais il ne sera plus synchronisé avec Microsoft.
device-detail-unmanage-action-confirm = Désinscrire

# Projects list (ProjectsView): workspace-wide grid of projects
# rendered from the sync engine pool, with status pills and a
# short description per card.
projects-list-heading = Projets
projects-list-subheading = Aperçu du moteur de synchronisation (option projects_v2).
projects-list-no-description = Aucune description

# Project detail (ProjectDetailView): per-project kanban board
# with a header, status pill, ticket count, and a Group-by
# control on the kanban toolbar.
project-detail-loading-name = Chargement…
project-detail-ticket-count = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
   }
project-detail-group-by-label = Grouper par
project-detail-group-by-status = Statut uniquement
project-detail-group-by-assignee = Statut x Assigné
project-detail-group-by-priority = Statut x Priorité
project-detail-loading = Chargement du projet…

# Project Gantt (ProjectGanttView): per-project Gantt timeline
# with a header summary of ticket and dependency-link counts.
project-gantt-fallback-name = Projet
project-gantt-summary = { $tickets ->
    [one] { $tickets } ticket
   *[other] { $tickets } tickets
   } · { $links ->
    [one] { $links } lien
   *[other] { $links } liens
   }

# Project cycles (ProjectCyclesView): full-page cycles surface
# with active-cycle burndown, create form, and a list of every
# cycle for the project (planned / active / completed).
project-cycles-fallback-name = Projet
project-cycles-count = { $count ->
    [one] { $count } cycle
   *[other] { $count } cycles
   }
project-cycles-new-button = Nouveau cycle
project-cycles-cancel-button = Annuler
project-cycles-date-missing = —
project-cycles-confirm-complete = Terminer ce cycle ? L'instantané sera figé.
project-cycles-confirm-archive = Archiver ce cycle ?
project-cycles-create-title = Nouveau cycle
project-cycles-field-name = Nom
project-cycles-field-start = Début
project-cycles-field-end = Fin
project-cycles-name-placeholder = par ex. Sprint 14
project-cycles-create-submit = Créer
project-cycles-all-title = Tous les cycles
project-cycles-empty-prefix = Aucun cycle pour l'instant. Cliquez sur
project-cycles-empty-cta = Nouveau cycle
project-cycles-empty-suffix = pour démarrer une itération.
project-cycles-state-planned = planifié
project-cycles-state-active = actif
project-cycles-state-completed = terminé
project-cycles-action-promote = Activer
project-cycles-action-complete = Terminer
project-cycles-action-archive = Archiver

# Workspace cycles (WorkspaceCyclesView): cross-project overview
# of in-flight iterations, grouped by project, with a toggle to
# pull completed cycles back into view.
workspace-cycles-heading = Cycles
workspace-cycles-subheading = Itérations en cours dans tous les projets
workspace-cycles-show-completed = Afficher les terminés
workspace-cycles-loading = Chargement des cycles…
workspace-cycles-error-fallback = Impossible de charger les cycles
workspace-cycles-empty-title = Aucun cycle pour l'instant.
workspace-cycles-empty-hint = Ouvrez un projet et lancez-en un depuis le panneau Cycles.
workspace-cycles-group-count = { $count ->
    [one] { $count } cycle
   *[other] { $count } cycles
   }
workspace-cycles-project-fallback = Projet n° { $id }
workspace-cycles-date-missing = —
workspace-cycles-state-planned = planifié
workspace-cycles-state-completed = terminé

# Cycle detail (CycleDetailView): Scrum board scoped to one
# cycle, with a burndown pinned above the kanban toolbar.
cycle-detail-back = ‹ Cycles
cycle-detail-loading-name = Chargement…
cycle-detail-loading = Chargement du cycle…
cycle-detail-error-fallback = Impossible de charger le cycle
cycle-detail-summary = { $state } · { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
   }
cycle-detail-group-by-label = Grouper par
cycle-detail-group-by-status = Statut uniquement
cycle-detail-group-by-assignee = Statut x Assigné
cycle-detail-group-by-priority = Statut x Priorité
cycle-detail-state-planned = Planifié
cycle-detail-state-active = Actif
cycle-detail-state-completed = Terminé

# Documentation index (DocumentationIndexView): hub page listing
# recently updated, starred, collections, and status chips.
docs-index-title = Documentation
docs-index-new-page = Nouvelle page
docs-index-recently-updated = Récemment mises à jour
docs-index-recently-updated-count = Dernières { $count }
docs-index-no-recent-activity = Aucune activité récente.
docs-index-starred = Favoris
docs-index-starred-hint = Marquez une page comme favori depuis son menu pour y accéder rapidement.
docs-index-browse-all = Parcourir toutes les pages
docs-index-chip-drafts = { $count ->
    [one] { $count } brouillon
   *[other] { $count } brouillons
   }
docs-index-chip-archived = { $count ->
    [one] { $count } archivée
   *[other] { $count } archivées
   }
docs-index-chip-trash = { $count ->
    [one] { $count } dans la corbeille
   *[other] { $count } dans la corbeille
   }

# Documentation drafts (DocumentationDraftsView): pages not yet
# assigned to a collection.
docs-drafts-title = Brouillons
docs-drafts-heading = Brouillons
docs-drafts-description = Pages non encore assignées à une collection
docs-drafts-back = Retour à la documentation
docs-drafts-count = { $count ->
    [one] { $count } page
   *[other] { $count } pages
   }

# Documentation archived (DocumentationArchivedView): list of
# pages that have been archived, with a restore action.
docs-archived-title = Archivées
docs-archived-heading = Archivées
docs-archived-description = Pages qui ont été archivées
docs-archived-back = Retour à la documentation
docs-archived-count = { $count ->
    [one] { $count } page
   *[other] { $count } pages
   }
docs-archived-loading = Chargement des pages archivées
docs-archived-archived-at = Archivée le { $date }
docs-archived-restore = Restaurer

# Documentation trash (DocumentationTrashView): deleted pages,
# with restore and permanent-delete actions.
docs-trash-title = Corbeille
docs-trash-heading = Corbeille
docs-trash-description = Les pages supprimées peuvent être restaurées ou supprimées définitivement
docs-trash-back = Retour à la documentation
docs-trash-count = { $count ->
    [one] { $count } page
   *[other] { $count } pages
   }
docs-trash-loading = Chargement des pages supprimées
docs-trash-deleted-at = Supprimée le { $date }
docs-trash-restore = Restaurer
docs-trash-delete-forever = Supprimer définitivement
docs-trash-confirm-delete = Confirmer la suppression ?

# Documentation gaps (DocumentationGapsView): queue of open
# knowledge gaps with a list pane and detail pane.
docs-gaps-title = Lacunes de connaissances
docs-gaps-heading = Lacunes de connaissances
docs-gaps-back-docs = Docs
docs-gaps-back-list = Lacunes de connaissances
docs-gaps-refresh = Actualiser les signaux
docs-gaps-refreshing = Actualisation
docs-gaps-detect-no-results = Aucun nouveau groupe trouvé
docs-gaps-detect-created = { $count } nouveau(x)
docs-gaps-detect-updated = { $count } mis à jour
docs-gaps-loading = Chargement
docs-gaps-empty = Aucune lacune de connaissances ouverte. Signalez un ticket depuis sa barre latérale pour en ajouter une.
docs-gaps-impact-searches = recherches
docs-gaps-impact-recent-tickets = tickets récents
docs-gaps-impact-tickets = tickets
docs-gaps-impact-tooltip = { $count } { $label } indiquant la demande pour ce document
docs-gaps-signal-count = { $count ->
    [one] { $count } signal
   *[other] { $count } signaux
   }
docs-gaps-select-prompt = Sélectionnez une lacune dans la liste pour voir ses preuves.
docs-gaps-status-label = Statut :
docs-gaps-last-evidence = Dernière preuve : { $time }
docs-gaps-dismiss = Ignorer
docs-gaps-evidence-heading = Preuves
docs-gaps-evidence-empty = Aucune preuve.
docs-gaps-signal-manual-flag = Signalement manuel
docs-gaps-signal-ticket-cluster = Groupe de tickets
docs-gaps-signal-failed-search = Recherche infructueuse
docs-gaps-signal-stale-doc = Document obsolète
docs-gaps-signal-ai-suggested = Suggestion IA
docs-gaps-cluster-fallback = Groupe
docs-gaps-cluster-via = via { $channel }
docs-gaps-cluster-more = { $count ->
    [one] et { $count } de plus
   *[other] et { $count } de plus
   }
docs-gaps-stale-untitled = Document sans titre
docs-gaps-stale-verified = Vérifié { $time }
docs-gaps-stale-verified-no-time = Vérifié
docs-gaps-stale-days-past-due = { $count ->
    [one] { $count } jour de retard
   *[other] { $count } jours de retard
   }
docs-gaps-stale-recent-tickets = { $count ->
    [one] ticket récemment clos cite encore ce document :
   *[other] tickets récemment clos citent encore ce document :
   }
docs-gaps-stale-plus-more = + { $count } de plus
docs-gaps-stale-auto-dismiss = Revérifier le document fermera automatiquement cette lacune.
docs-gaps-failed-search-count = { $count ->
    [one] { $count } recherche sans résultat
   *[other] { $count } recherches sans résultat
   }
docs-gaps-failed-search-range = première { $first }, dernière { $last }
docs-gaps-flagged-by = Signalé par { $name }
docs-gaps-resolve-heading = Résoudre cette lacune
docs-gaps-resolve-body = Ouvrez l'un des tickets ci-dessus et utilisez { $action } dans sa barre latérale. Le nouveau document sera automatiquement lié comme 'résout' à chaque ticket signalé.
docs-gaps-resolve-action = Enregistrer comme document

# Document view (DocumentView): full-page editor for a single doc
# page (or a ticket note). Covers the header toolbar, metadata
# strip, save indicators, verification chips, panels, and toasts.
doc-detail-back-to-ticket = Retour au ticket
doc-detail-back-to-documentation = Retour à la documentation
doc-detail-saving = Enregistrement
doc-detail-publish = Publier
doc-detail-star = Mettre en favori
doc-detail-unstar = Retirer des favoris
doc-detail-copy-link = Copier le lien
doc-detail-copied = Copié
doc-detail-untitled = Sans titre
doc-detail-status-draft = Brouillon
doc-detail-status-archived = Archivé
doc-detail-needs-verification = Vérification requise
doc-detail-needs-verification-title = Vérifier cette page
doc-detail-verification-stale = Vérification expirée
doc-detail-verification-stale-title = Revérifier cette page
doc-detail-sse-live = Mises à jour en direct actives
doc-detail-sse-connecting = Connexion en cours
doc-detail-sse-disconnected = Déconnecté
doc-detail-history = Historique
doc-detail-history-title = Historique des révisions
doc-detail-editor-placeholder = Saisissez le contenu de la documentation ici
doc-detail-not-found-title = Document introuvable
doc-detail-not-found-body = Le document que vous recherchez n'existe pas ou a été déplacé.
doc-detail-not-found-link = Accéder à l'accueil de la documentation
doc-detail-toast-deleting = Suppression du document
doc-detail-toast-deleted = Document supprimé avec succès
doc-detail-toast-delete-error = Erreur lors de la suppression du document
doc-detail-duplicate-suffix = { $title } (copie)
doc-detail-ticket-note-title = Notes pour le ticket n°{ $id }
doc-detail-ticket-note-description = Documentation pour le ticket { $title }
doc-detail-ticket-note-author-system = Système

# Asset planner (AssetPlannerView): kanban de planification des
# déploiements d'équipements, regroupés par famille d'OS, période
# de garantie ou état de conformité. Couvre l'en-tête, les filtres
# latéraux, les colonnes et les puces des cartes.
asset-planner-title = Équipements
asset-planner-subtitle = Planifiez les déploiements par OS, garantie ou conformité.
asset-planner-search-placeholder = Rechercher par nom, hôte, modèle…
asset-planner-group-by = Grouper par
asset-planner-axis-os = Famille d'OS
asset-planner-axis-warranty = Garantie
asset-planner-axis-compliance = Conformité
asset-planner-loading = Chargement des équipements…
asset-planner-load-error = Échec du chargement des équipements
asset-planner-filters-heading = Filtres
asset-planner-filters-clear = Effacer ({ $count })
asset-planner-section-os = OS
asset-planner-section-warranty = Garantie
asset-planner-section-compliance = Conformité
asset-planner-count = { $visible } sur { $total ->
    [one] { $total } équipement
   *[other] { $total } équipements
   }
asset-planner-empty = Aucun équipement ne correspond aux filtres actuels.
asset-planner-warranty-ends = Garantie expire le { $date }
asset-planner-no-warranty-data = Pas de donnée de garantie
asset-planner-warranty-unknown-short = n/d
asset-planner-card-host = Hôte
asset-planner-card-os = OS
asset-planner-card-model = Modèle
asset-planner-card-tag = Étiquette
asset-planner-card-compliance = Conformité
asset-planner-os-windows = Windows
asset-planner-os-macos = macOS
asset-planner-os-linux = Linux
asset-planner-os-ios = iOS
asset-planner-os-android = Android
asset-planner-os-other = Autre
asset-planner-warranty-expired = Expirée
asset-planner-warranty-expiring-30d = Expire dans 30 jours
asset-planner-warranty-expiring-90d = Expire dans 90 jours
asset-planner-warranty-active = Active
asset-planner-warranty-unknown = Inconnue
asset-planner-compliance-unknown = Inconnue

# Collection view (CollectionView): documentation collection
# detail page with editable name/icon, an overview editor,
# visibility chips, an expandable list of pages with custom
# permissions, and the collection's page tree.
collection-back-to-documentation = Retour à la documentation
collection-not-found-title = Collection introuvable
collection-action-delete = Supprimer
collection-action-manage-access = Gérer l'accès
collection-action-new-page = Nouvelle page
collection-new-page-default-title = Nouvelle page
collection-not-found-heading = Collection introuvable
collection-not-found-description = Cette collection a peut-être été déplacée ou supprimée.
collection-badge-system = Système
collection-badge-restricted = Restreinte
collection-badge-public = Publique
collection-overview-heading = Aperçu
collection-overview-placeholder = Rédigez un aperçu pour cette collection...
collection-overrides-summary = { $count ->
    [one] { $count } page avec autorisations personnalisées
   *[other] { $count } pages avec autorisations personnalisées
   }
collection-pages-heading = Pages
collection-page-count = { $count ->
    [one] { $count } page
   *[other] { $count } pages
   }
collection-delete-title = Supprimer { $name } ?
collection-delete-title-fallback = Supprimer la collection ?
collection-delete-message = Les pages de cette collection ne seront pas supprimées.
collection-delete-confirm = Supprimer

# CSV import (CsvImportView): admin page for importing users,
# devices, or tickets from CSV. Covers the page header, status
# messages, action buttons, the import status card, guideline
# panels, the template list, and both modals (file upload and
# template download).
csv-import-back = Retour à l'import de données
csv-import-title = Import CSV
csv-import-subtitle = Importez des données depuis des fichiers CSV dans votre système
csv-import-action-import = Importer des données
csv-import-action-templates = Télécharger les modèles
csv-import-status-heading = Statut de l'import
csv-import-status-success = Import terminé
csv-import-status-in-progress = Import en cours
csv-import-status-error = Échec de l'import
csv-import-last-import = Dernier import : { $date }
csv-import-results-total = Enregistrements au total
csv-import-results-successful = Réussis
csv-import-results-failed = Échoués
csv-import-guidelines-heading = Consignes pour l'import CSV
csv-import-requirements-heading = Prérequis du fichier CSV
csv-import-requirements-utf8 = Les fichiers doivent être au format CSV avec encodage UTF-8
csv-import-requirements-headers = La première ligne doit contenir les en-têtes de colonnes correspondant aux champs attendus
csv-import-requirements-required = Les champs obligatoires ne doivent pas être vides
csv-import-requirements-date-format = Les champs de date doivent utiliser le format AAAA-MM-JJ
csv-import-requirements-max-size = Taille maximale du fichier : 10 Mo
csv-import-notes-heading = Notes importantes
csv-import-notes-updates = Les enregistrements existants seront mis à jour s'ils partagent un identifiant unique (comme l'e-mail ou l'ID)
csv-import-notes-validation = La validation des données est effectuée avant l'import, les enregistrements contenant des données invalides seront ignorés
csv-import-notes-duration = Pour les imports volumineux, le processus peut prendre plusieurs minutes
csv-import-notes-templates = Téléchargez et utilisez nos modèles pour garantir un formatage correct
csv-import-templates-heading = Modèles disponibles
csv-import-templates-intro = Utilisez ces modèles comme point de départ pour vos imports CSV
csv-import-template-users-name = Modèle Utilisateurs
csv-import-template-users-description = Importez des comptes utilisateur avec rôles et coordonnées
csv-import-template-devices-name = Modèle Appareils
csv-import-template-devices-description = Importez des appareils avec détails matériels et informations de propriété
csv-import-template-tickets-name = Modèle Tickets
csv-import-template-tickets-description = Importez des tickets de support avec détails et personnes assignées
csv-import-template-download = Télécharger
csv-import-modal-import-title = Importer des données depuis un CSV
csv-import-modal-data-type = Type de données
csv-import-modal-type-users = Utilisateurs
csv-import-modal-type-devices = Appareils
csv-import-modal-type-tickets = Tickets
csv-import-modal-file-label = Fichier CSV
csv-import-modal-upload-link = Téléverser un fichier
csv-import-modal-drag-drop = ou glissez-déposez
csv-import-modal-size-hint = Fichiers CSV jusqu'à 10 Mo
csv-import-modal-cancel = Annuler
csv-import-modal-start = Lancer l'import
csv-import-modal-starting = Import en cours...
csv-import-modal-templates-title = Modèles CSV
csv-import-modal-templates-intro = Téléchargez nos modèles CSV pour vous assurer que vos données sont correctement formatées pour l'import.
csv-import-modal-fields-count = { $count ->
    [one] { $count } champ
   *[other] { $count } champs
   }
csv-import-modal-close = Fermer
csv-import-error-no-file = Veuillez sélectionner un fichier à importer
csv-import-error-failed = Échec de l'import
csv-import-error-generic = Impossible d'importer les données
csv-import-success-completed = Import terminé avec succès
csv-import-toast-template-downloaded = Modèle { $type } téléchargé

# Error page (ErrorView)
error-page-default-code = 404
error-page-default-message = Page introuvable
error-page-description = La page que vous cherchez n'existe pas ou vous n'y avez peut-être pas accès.
error-page-go-back = Retour
error-page-go-home = Aller au tableau de bord
error-page-debug-title = Contrôles de débogage (appuyez sur « d » pour basculer)
error-page-debug-master-toggle = Activation générale des effets
error-page-debug-global-intensity = Intensité globale
error-page-debug-channel-separation = Séparation des canaux
error-page-debug-distortion-scale = Échelle de distorsion
error-page-debug-glitch-frequency = Fréquence des glitchs
error-page-debug-glitch-intensity = Intensité des glitchs
error-page-debug-cursor-influence = Influence du curseur

# PDF viewer (PDFViewerView)
pdf-viewer-default-filename = Document
pdf-viewer-back = Retour
pdf-viewer-share = Partager
pdf-viewer-share-tooltip = Copier le lien dans le presse-papiers
pdf-viewer-loading = Chargement du document PDF...
pdf-viewer-error-title = Échec du chargement du PDF
pdf-viewer-error-go-back = Retour
pdf-viewer-error-no-source = Aucune source PDF fournie
pdf-viewer-error-failed = Échec du chargement du PDF
pdf-viewer-error-failed-with-reason = Échec du chargement du PDF : { $reason }
pdf-viewer-error-unknown = Erreur inconnue

# Settings: MFA (MFASettings) - configuration, vérification, codes de secours et désactivation de la double authentification
settings-mfa-title = Authentification à deux facteurs
settings-mfa-title-success = Configuration terminée !
settings-mfa-toggle-label = Activer l'authentification à deux facteurs
settings-mfa-toggle-description-enabled = Votre compte est protégé par l'A2F
settings-mfa-toggle-description-disabled = Sécurisez votre compte avec une application d'authentification
settings-mfa-admin-status-enabled = Activée
settings-mfa-admin-status-disabled = Désactivée
settings-mfa-admin-backup-codes-generated = · Codes de secours générés
settings-mfa-admin-disable = Désactiver
settings-mfa-admin-disabling = Désactivation...
settings-mfa-admin-note = La configuration de l'A2F nécessite l'application d'authentification du titulaire du compte.
settings-mfa-admin-disable-success = L'authentification à deux facteurs a été désactivée pour cet utilisateur
settings-mfa-admin-disable-error = Échec de la désactivation de l'A2F
settings-mfa-admin-load-error = Échec du chargement du statut A2F de cet utilisateur
settings-mfa-setup-init-error = Échec de l'initialisation de la configuration A2F
settings-mfa-setup-not-ready = La configuration A2F n'est pas correctement initialisée
settings-mfa-manual-toggle = Impossible de scanner ? Saisissez le code manuellement
settings-mfa-manual-instructions = Saisissez cette clé secrète dans votre application d'authentification :
settings-mfa-copy-button = Copier
settings-mfa-copied-button = Copié !
settings-mfa-copy-tooltip = Copier dans le presse-papiers
settings-mfa-copied-tooltip = Copié dans le presse-papiers !
settings-mfa-copy-error = Échec de la copie dans le presse-papiers
settings-mfa-verify-heading = Saisissez le code de vérification
settings-mfa-verify-instructions = Saisissez le code à 6 chiffres de votre application d'authentification :
settings-mfa-verify-aria-label = Code de vérification A2F
settings-mfa-verify-button = Vérifier
settings-mfa-verifying-button = Vérification...
settings-mfa-verify-invalid-length = Veuillez saisir un code valide à 6 chiffres
settings-mfa-verify-missing-secret = La clé secrète A2F est manquante. Veuillez recommencer la configuration.
settings-mfa-verify-invalid-code = Code de vérification invalide. Veuillez réessayer.
settings-mfa-verify-incomplete-login = A2F activée, mais la réponse de connexion est incomplète
settings-mfa-qr-alt = Code QR A2F
settings-mfa-verifying-heading = Vérification du code
settings-mfa-verifying-message = Veuillez patienter pendant la vérification de votre code d'authentification...
settings-mfa-disable-password-prompt = Veuillez saisir votre mot de passe pour désactiver l'A2F :
settings-mfa-backup-codes-heading = Codes de secours
settings-mfa-backup-codes-description = Conservez ces codes de secours en lieu sûr. Vous pouvez les utiliser pour accéder à votre compte en cas de perte de votre appareil d'authentification.
settings-mfa-backup-codes-download = Télécharger
settings-mfa-backup-codes-download-tooltip = Télécharger les codes de secours au format texte
settings-mfa-backup-codes-download-success = Codes de secours téléchargés avec succès
settings-mfa-backup-codes-download-error = Échec du téléchargement des codes de secours
settings-mfa-backup-file-title = Codes de secours Nosdesk
settings-mfa-backup-file-warning = IMPORTANT : conservez ces codes de secours en lieu sûr.
settings-mfa-backup-file-usage = Chaque code ne peut être utilisé qu'une seule fois pour accéder à votre compte en cas de perte de votre appareil d'authentification.
settings-mfa-backup-file-codes-heading = Codes de secours :
settings-mfa-backup-file-generated = Généré le : { $date }
settings-mfa-success-heading = Authentification à deux facteurs activée !
settings-mfa-success-message = Votre compte est désormais protégé par l'A2F. Vous devrez saisir un code de votre application d'authentification à chaque connexion.
settings-mfa-success-cta = Commencer à utiliser Nosdesk !

# Settings: auth methods (AuthMethodsSettings) - fournisseurs de connexion liés et gestion des sessions actives
settings-auth-methods-section-title = Méthodes d'authentification
settings-auth-methods-type-local = E-mail / Mot de passe
settings-auth-methods-type-microsoft = Microsoft
settings-auth-methods-primary-badge = Principale
settings-auth-methods-added-suffix = · Ajoutée le { $date }
settings-auth-methods-remove = Supprimer
settings-auth-methods-connect-microsoft = Connecter un compte Microsoft
settings-auth-methods-connect-microsoft-already = Déjà connecté
settings-auth-methods-connect-microsoft-provider = Azure AD / Entra ID
settings-auth-methods-link-success = Compte { $provider } associé avec succès
settings-auth-methods-link-error = Échec de l'association du compte { $provider }
settings-auth-methods-remove-success = Méthode d'authentification supprimée avec succès
settings-auth-methods-remove-error = Échec de la suppression de la méthode d'authentification
settings-auth-methods-sessions-section-title = Sessions actives
settings-auth-methods-sessions-revoke-all = Révoquer toutes les autres
settings-auth-methods-sessions-unknown-device = Appareil inconnu
settings-auth-methods-sessions-unknown-location = Emplacement inconnu
settings-auth-methods-sessions-current-badge = Actuelle
settings-auth-methods-sessions-last-active = { $location } • Dernière activité { $date }
settings-auth-methods-sessions-revoke = Révoquer
settings-auth-methods-sessions-revoke-success = Session révoquée avec succès
settings-auth-methods-sessions-revoke-error = Échec de la révocation de la session
settings-auth-methods-sessions-revoke-all-success = Toutes les autres sessions ont été révoquées
settings-auth-methods-sessions-revoke-all-error = Échec de la révocation des sessions
settings-auth-methods-sessions-load-error = Échec du chargement des sessions actives

# Habillage commun.
common-modal-close = Fermer la fenêtre

# Éditeur : barre d'outils (CollaborativeEditor)
editor-toolbar-text-style = Style de texte
editor-toolbar-bold = Gras
editor-toolbar-italic = Italique
editor-toolbar-bullet-list = Liste à puces
editor-toolbar-numbered-list = Liste numérotée
editor-toolbar-insert = Insérer
editor-toolbar-undo = Annuler
editor-toolbar-redo = Rétablir
editor-toolbar-revision-history = Historique des révisions
editor-toolbar-editing-with = Modification avec :
editor-toolbar-connection-connecting = Connexion...
editor-toolbar-connection-disconnected = Déconnecté
editor-toolbar-user-title = { $name }
editor-toolbar-user-title-uuid = { $name } (UUID : { $uuid })

# Éditeur : menu de style de texte (CollaborativeEditor)
editor-type-menu-plain = Texte simple
editor-type-menu-heading-1 = Titre 1
editor-type-menu-heading-2 = Titre 2
editor-type-menu-heading-3 = Titre 3
editor-type-menu-blockquote = Citation
editor-type-menu-code-block = Bloc de code

# Éditeur : menu d'insertion (CollaborativeEditor)
editor-insert-menu-bullet-list = Liste à puces
editor-insert-menu-numbered-list = Liste numérotée
editor-insert-menu-blockquote = Citation
editor-insert-menu-code-block = Bloc de code
editor-insert-menu-link = Lien
editor-insert-menu-embed-document = Intégrer un document

# Éditeur : invite de langage du bloc de code (CollaborativeEditor)
editor-code-block-language-prompt = Langage pour la coloration syntaxique (facultatif) :

# Éditeur : menu de mentions (CollaborativeEditor)
editor-mention-searching = Recherche de « { $query } »
editor-mention-no-results = Aucun utilisateur trouvé
editor-mention-hint-navigate = Naviguer
editor-mention-hint-select = Sélectionner
editor-mention-hint-close = Fermer

# Éditeur : info-bulle de lien (LinkTooltip)
editor-link-tooltip-placeholder = Saisir une URL...
editor-link-tooltip-apply = Appliquer
editor-link-tooltip-cancel = Annuler
editor-link-tooltip-edit = Modifier le lien
editor-link-tooltip-remove = Supprimer le lien

# Éditeur : sélecteur de document (DocumentPicker)
editor-doc-picker-title = Intégrer un document
editor-doc-picker-close = Fermer
editor-doc-picker-search-placeholder = Rechercher des documents...
editor-doc-picker-empty = Aucun document trouvé.

# Éditeur : panneau d'historique (RevisionHistory)
editor-revision-history-title = Historique des révisions

# Éditeur : liste des révisions (RevisionList)
editor-revisions-empty-title = Aucune révision pour l'instant
editor-revisions-empty-hint = Les révisions sont créées lorsque vous apportez des modifications
editor-revisions-current-version = Version actuelle
editor-revisions-by = Par :
editor-revisions-more-contributors = +{ $count }
editor-revisions-word-count = { $count } { $count ->
    [one] mot
   *[other] mots
  }
editor-revisions-restore-button = Restaurer cette version
editor-revisions-restoring = Restauration...
editor-revisions-unknown-user = Inconnu
editor-revisions-load-error = Échec du chargement des révisions
editor-revisions-restore-error = Échec de la restauration de la révision
editor-revisions-just-now = À l'instant
editor-revisions-minutes-ago = il y a { $minutes } min
editor-revisions-hours-ago = il y a { $hours } h
editor-revisions-days-ago = il y a { $days } j
editor-revisions-confirm-title = Restaurer la révision ?
editor-revisions-confirm-body = Le ticket sera restauré à la révision { $revision }. Cette action remplacera le contenu actuel par celui de la révision sélectionnée.
editor-revisions-confirm-note = Remarque : une nouvelle révision sera créée afin que vous puissiez toujours annuler cette opération.
editor-revisions-confirm-cancel = Annuler
editor-revisions-confirm-restore = Restaurer
# Ticket media: aperçu de pièce jointe (AttachmentPreview)
ticket-media-attachment-voice-message = Message vocal
ticket-media-attachment-file-fallback = Fichier
ticket-media-attachment-pdf-document = Document PDF.{ $ext }
ticket-media-attachment-video = Vidéo.{ $ext }
ticket-media-attachment-audio = Audio.{ $ext }
ticket-media-attachment-image = Image.{ $ext }
ticket-media-attachment-file = Fichier.{ $ext }
ticket-media-attachment-download = Télécharger la pièce jointe
ticket-media-attachment-download-image = Télécharger l'image
ticket-media-attachment-download-animated = Télécharger l'image animée
ticket-media-attachment-download-pdf = Télécharger le PDF
ticket-media-attachment-delete-audio = Supprimer l'audio
ticket-media-attachment-delete-video = Supprimer la vidéo
ticket-media-attachment-delete-image = Supprimer l'image
ticket-media-attachment-delete-pdf = Supprimer le PDF
ticket-media-attachment-delete-file = Supprimer le fichier
ticket-media-attachment-format-unsupported = Ce format d'image n'est pas pris en charge par votre navigateur
ticket-media-attachment-loading-pdf = Chargement du PDF
ticket-media-attachment-animated-badge = ANIMÉ
ticket-media-attachment-cancel = Annuler
ticket-media-attachment-submit-video = Envoyer la vidéo
ticket-media-attachment-preview-title-animated = Aperçu de l'image animée
ticket-media-attachment-preview-title-image = Aperçu de l'image

# Ticket media: lecteur audio (AudioPlayer)
ticket-media-audio-play = Lecture
ticket-media-audio-pause = Pause
ticket-media-audio-loading = Chargement...
ticket-media-audio-transcription = Transcription

# Ticket media: dictaphone (VoiceRecorder)
ticket-media-voice-recording = Enregistrement
ticket-media-voice-cancel = Annuler
ticket-media-voice-stop = Arrêter l'enregistrement
ticket-media-voice-mic-error = Impossible d'accéder au microphone. Veuillez vérifier vos autorisations.

# Ticket media: lecteur vidéo (VideoPlayer)
ticket-media-video-play = Lecture
ticket-media-video-pause = Pause
ticket-media-video-mute = Muet
ticket-media-video-unmute = Réactiver le son
ticket-media-video-fullscreen-enter = Passer en plein écran
ticket-media-video-fullscreen-exit = Quitter le plein écran

# Ticket media: visionneuse PDF (PDFViewer)
ticket-media-pdf-aria = Visionneuse PDF
ticket-media-pdf-loading = Chargement du PDF...
ticket-media-pdf-zoom-out = Zoom arrière
ticket-media-pdf-zoom-out-aria = Zoom arrière
ticket-media-pdf-zoom-in = Zoom avant
ticket-media-pdf-zoom-in-aria = Zoom avant
ticket-media-pdf-fit-width = Ajuster à la largeur
ticket-media-pdf-fit-width-aria = Ajuster à la largeur
ticket-media-pdf-fullscreen = Plein écran
ticket-media-pdf-fullscreen-aria = Ouvrir en plein écran
ticket-media-pdf-download = Télécharger le PDF
ticket-media-pdf-download-aria = Télécharger le PDF

# Ticket media: aperçu de fichier (FilePreview)
ticket-media-file-fallback = Fichier
ticket-media-file-pdf = Document PDF.{ $ext }
ticket-media-file-word = Document Word.{ $ext }
ticket-media-file-excel = Feuille Excel.{ $ext }
ticket-media-file-powerpoint = Présentation.{ $ext }
ticket-media-file-image = Image.{ $ext }
ticket-media-file-archive = Archive.{ $ext }
ticket-media-file-text = Document texte.{ $ext }
ticket-media-file-generic = Fichier.{ $ext }
ticket-media-file-delete = Supprimer le fichier
ticket-media-file-download = Télécharger
ticket-media-file-thumbnail-error = Échec de la génération de la miniature
ticket-media-file-image-error = Échec du chargement de l'image
ticket-media-file-animated-badge = ANIMÉ

# Ticket picker: sélecteur d'utilisateur (UserPicker)
ticket-picker-user-placeholder-assignee = Attribuer à...
ticket-picker-user-placeholder-requester = Trouver un utilisateur...
ticket-picker-user-search-staff = Rechercher du personnel...
ticket-picker-user-search-users = Rechercher des utilisateurs...
ticket-picker-user-sheet-title-assignee = Attribuer à
ticket-picker-user-sheet-title-requester = Trouver un utilisateur
ticket-picker-user-listbox-assignees = Utilisateurs assignables
ticket-picker-user-listbox-users = Utilisateurs
ticket-picker-user-loading-assignee = Chargement des assignataires
ticket-picker-user-loading-requester = Chargement des demandeurs
ticket-picker-user-view-profile = Voir le profil de { $name }
ticket-picker-user-clear = Effacer la sélection
ticket-picker-user-empty-assignees = Aucun utilisateur assignable pour le moment.
ticket-picker-user-empty-users = Aucun utilisateur trouvé.
ticket-picker-user-empty-search = Aucun utilisateur ne correspond à "{ $query }"
ticket-picker-user-section-selected-assignee = Actuellement assigné
ticket-picker-user-section-selected-requester = Demandeur actuel
ticket-picker-user-section-you = Vous
ticket-picker-user-section-recent = Récents
ticket-picker-user-section-results = Résultats
ticket-picker-user-section-staff = Personnel
ticket-picker-user-section-all = Tous les utilisateurs
ticket-picker-user-you-suffix = (vous)

# Ticket picker: modale de ticket lié (LinkedTicketModal)
ticket-picker-linked-title = Lier un ticket
ticket-picker-linked-search-placeholder = Rechercher des tickets...
ticket-picker-linked-loading = Chargement des tickets...
ticket-picker-linked-error = Échec du chargement des tickets
ticket-picker-linked-try-again = Réessayer
ticket-picker-linked-empty-search = Aucun ticket ne correspond à votre recherche
ticket-picker-linked-empty = Aucun ticket à lier
ticket-picker-linked-col-id = ID
ticket-picker-linked-col-title = Titre
ticket-picker-linked-col-status = Statut
ticket-picker-linked-col-requester = Demandeur
ticket-picker-linked-col-updated = Mis à jour
ticket-picker-linked-count = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
}
ticket-picker-linked-cancel = Annuler

# Ticket picker: réponses préenregistrées (CannedResponsePicker)
ticket-picker-canned-trigger-aria = Insérer une réponse préenregistrée
ticket-picker-canned-trigger-title = Insérer une réponse préenregistrée ({ $shortcut })
ticket-picker-canned-listbox-aria = Réponses préenregistrées
ticket-picker-canned-loading = Chargement…
ticket-picker-canned-empty-title = Aucune réponse préenregistrée pour le moment.
ticket-picker-canned-empty-hint = Les administrateurs peuvent ajouter des modèles dans la zone d'administration.
ticket-picker-canned-load-error = Échec du chargement des modèles

# Ticket picker: modale d'appareil (DeviceModal)
ticket-picker-device-title = Ajouter un appareil
ticket-picker-device-name-label = Nom
ticket-picker-device-name-placeholder = Entrez le nom de l'appareil
ticket-picker-device-hostname-label = Nom d'hôte
ticket-picker-device-hostname-placeholder = Entrez le nom d'hôte
ticket-picker-device-serial-label = Numéro de série
ticket-picker-device-serial-placeholder = Entrez le numéro de série
ticket-picker-device-model-label = Modèle
ticket-picker-device-model-placeholder = Entrez le modèle
ticket-picker-device-warranty-label = État de la garantie
ticket-picker-device-warranty-active = Active
ticket-picker-device-warranty-warning = Avertissement
ticket-picker-device-warranty-expired = Expirée
ticket-picker-device-warranty-unknown = Inconnu
ticket-picker-device-cancel = Annuler
ticket-picker-device-add = Ajouter l'appareil

# Sélecteur d'icône de document (DocumentIconSelector)
doc-icon-selector-trigger-aria = Sélectionner une icône de document
doc-icon-selector-search-placeholder = Rechercher des icônes...
doc-icon-selector-empty = Aucune icône trouvée
doc-icon-selector-footer-hint = Cliquez sur une icône pour la sélectionner
doc-icon-selector-random = Aléatoire
doc-icon-selector-scroll-dot-aria = Faire défiler vers la section { $index }
doc-icon-selector-category-suggested = Suggéré
doc-icon-selector-category-documents = Documents
doc-icon-selector-category-objects = Objets
doc-icon-selector-category-symbols = Symboles
doc-icon-selector-category-nature = Nature
doc-icon-selector-category-animals = Animaux
doc-icon-selector-category-people = Personnes
doc-icon-selector-category-travel = Voyage
doc-icon-selector-category-food = Nourriture
doc-icon-selector-category-activities = Activités
# Paramètres : profil (UserProfileCard)
settings-profile-banner-alt = Bannière de profil
settings-profile-change-photo = Changer la photo
settings-profile-name-placeholder = Saisissez un nom...
settings-profile-pronouns-label = Pronoms
settings-profile-pronouns-placeholder = Ajouter des pronoms (par ex. il/lui, elle/elle, iel)
settings-profile-save = Enregistrer
settings-profile-signature-label = Signature e-mail
settings-profile-signature-hint-prefix = Ajoutée à vos réponses sortantes sur les tickets issus d'un canal (e-mail). Le séparateur standard est
settings-profile-signature-hint-suffix = .
settings-profile-signature-placeholder = Nom du technicien
    Support informatique
settings-profile-unknown-user = Utilisateur inconnu
settings-profile-role-developer = Développeur
settings-profile-role-admin = Administrateur
settings-profile-role-technician = Technicien
settings-profile-role-user = Utilisateur
settings-profile-error-invalid-file = Fichier non valide
settings-profile-error-process-image = Impossible de traiter l'image
settings-profile-error-user-uuid-missing = UUID utilisateur introuvable
settings-profile-error-not-authenticated = Utilisateur non authentifié
settings-profile-avatar-upload-success = Photo de profil mise à jour avec succès
settings-profile-banner-upload-success = Image de couverture mise à jour avec succès
settings-profile-avatar-upload-error = Échec du téléversement de l'avatar
settings-profile-banner-upload-error = Échec du téléversement de la bannière
settings-profile-avatar-update-error = Échec de la mise à jour de l'avatar
settings-profile-banner-update-error = Échec de la mise à jour de la bannière
settings-profile-name-update-success = Nom mis à jour avec succès
settings-profile-name-update-error = Échec de la mise à jour du nom
settings-profile-pronouns-update-success = Pronoms mis à jour avec succès
settings-profile-pronouns-update-error = Échec de la mise à jour des pronoms
settings-profile-signature-update-success = Signature mise à jour
settings-profile-signature-update-error = Échec de la mise à jour de la signature

# Paramètres : notifications (NotificationSettings)
settings-notifications-category-ticket-label = Tickets
settings-notifications-category-ticket-description = Notifications concernant les attributions et changements de statut des tickets
settings-notifications-category-comment-label = Commentaires
settings-notifications-category-comment-description = Notifications lorsque quelqu'un commente vos tickets
settings-notifications-category-mention-label = Mentions
settings-notifications-category-mention-description = Notifications lorsque quelqu'un vous mentionne
settings-notifications-category-documentation-label = Documentation
settings-notifications-category-documentation-description = Notifications concernant les mises à jour des pages de documentation
settings-notifications-channel-in-app-name = Dans l'application
settings-notifications-channel-in-app-description = Notifications toast pendant l'utilisation de l'application
settings-notifications-channel-email-name = E-mail
settings-notifications-channel-email-description = Notifications par e-mail (limitées en fréquence)
settings-notifications-type-ticket-assigned-name = Ticket attribué
settings-notifications-type-ticket-assigned-description = Lorsqu'un ticket vous est attribué
settings-notifications-type-ticket-status-changed-name = Statut modifié
settings-notifications-type-ticket-status-changed-description = Lorsqu'un ticket auquel vous participez change de statut
settings-notifications-type-comment-added-name = Nouveau commentaire
settings-notifications-type-comment-added-description = Lorsque quelqu'un commente votre ticket
settings-notifications-type-mentioned-name = Mention
settings-notifications-type-mentioned-description = Lorsque quelqu'un vous mentionne dans un commentaire
settings-notifications-type-ticket-created-requester-name = Ticket créé
settings-notifications-type-ticket-created-requester-description = Lorsqu'un ticket est créé en votre nom
settings-notifications-type-doc-page-updated-name = Page mise à jour
settings-notifications-type-doc-page-updated-description = Lorsqu'une page de documentation à laquelle vous êtes abonné est modifiée
settings-notifications-browser-banner-title = Activer les notifications du navigateur
settings-notifications-browser-banner-description = Autorisez les notifications du navigateur pour recevoir des alertes même lorsque l'application n'est pas active.
settings-notifications-browser-banner-enable = Activer les notifications
settings-notifications-browser-enabled-success = Notifications du navigateur activées
settings-notifications-browser-denied-error = Autorisation de notification du navigateur refusée
settings-notifications-quick-settings-title = Réglages rapides
settings-notifications-channel-toggle-all-label = Toutes les notifications { $channel }
settings-notifications-column-header = Notification
settings-notifications-load-error = Échec du chargement des préférences de notification
settings-notifications-preference-update-success = Préférence mise à jour
settings-notifications-preference-update-error = Échec de la mise à jour de la préférence
settings-notifications-channel-bulk-success = Toutes les notifications { $channel } { $state ->
    [enabled] activées
   *[disabled] désactivées
}
settings-notifications-info-footer = Les notifications par e-mail sont limitées en fréquence pour éviter de saturer votre boîte de réception. Vous recevrez au plus un e-mail par ticket toutes les 5 minutes.

# Paramètres : clés d'accès (PasskeySettings)
settings-passkey-section-title = Clés d'accès
settings-passkey-empty-title = Aucune clé d'accès enregistrée
settings-passkey-empty-admin-description = Cet utilisateur n'a enregistré aucune clé d'accès.
settings-passkey-empty-self-description = Connectez-vous avec la biométrie ou une clé de sécurité au lieu d'un mot de passe
settings-passkey-add-button = Ajouter une clé d'accès
settings-passkey-add-another-button = Ajouter une autre clé d'accès
settings-passkey-synced-badge = Synchronisée
settings-passkey-last-used = Dernière utilisation { $date }
settings-passkey-never-used = Jamais utilisée
settings-passkey-rename-tooltip = Renommer la clé d'accès
settings-passkey-delete-tooltip = Supprimer la clé d'accès
settings-passkey-admin-info = L'enregistrement d'une clé d'accès requiert la biométrie ou la clé de sécurité du propriétaire du compte.
settings-passkey-unsupported-title = Navigateur non pris en charge
settings-passkey-unsupported-description = Votre navigateur ne prend pas en charge les clés d'accès (WebAuthn). Veuillez utiliser un navigateur récent comme Chrome, Safari, Firefox ou Edge.
settings-passkey-admin-load-error = Échec du chargement des clés d'accès pour cet utilisateur
settings-passkey-admin-delete-success = Clé d'accès supprimée
settings-passkey-admin-delete-error = Échec de la suppression de la clé d'accès
settings-passkey-add-modal-title = Ajouter une clé d'accès
settings-passkey-add-modal-description = Donnez un nom à votre clé d'accès pour la retrouver plus tard. Votre appareil vous invitera à créer la clé d'accès.
settings-passkey-add-modal-name-label = Nom de la clé d'accès (facultatif)
settings-passkey-add-modal-name-placeholder = par ex. MacBook Pro, iPhone
settings-passkey-modal-cancel = Annuler
settings-passkey-add-modal-create = Créer la clé d'accès
settings-passkey-add-modal-creating = Création...
settings-passkey-rename-modal-title = Renommer la clé d'accès
settings-passkey-rename-modal-name-label = Nom de la clé d'accès
settings-passkey-rename-modal-placeholder = Saisissez un nouveau nom
settings-passkey-rename-modal-save = Enregistrer
settings-passkey-delete-modal-title = Supprimer la clé d'accès
settings-passkey-delete-modal-confirm-prefix = Voulez-vous vraiment supprimer
settings-passkey-delete-modal-confirm-suffix = ? Vous ne pourrez plus utiliser cette clé d'accès pour vous connecter.
settings-passkey-delete-modal-password-label = Saisissez votre mot de passe pour confirmer
settings-passkey-delete-modal-password-placeholder = Votre mot de passe
settings-passkey-delete-modal-confirm = Supprimer la clé d'accès
settings-passkey-admin-delete-modal-confirm-prefix = Voulez-vous vraiment supprimer
settings-passkey-admin-delete-modal-confirm-suffix = ? Cet utilisateur ne pourra plus se connecter avec cette clé d'accès.
settings-passkey-admin-delete-modal-deleting = Suppression...

# Authentification : configuration de clé d'accès (PasskeySetup)
auth-passkey-setup-unsupported-title = Clés d'accès indisponibles
auth-passkey-setup-unsupported-insecure = Les clés d'accès nécessitent une connexion sécurisée (HTTPS). Vous êtes actuellement sur une connexion non sécurisée.
auth-passkey-setup-unsupported-browser = Votre navigateur ne prend pas en charge les clés d'accès. Veuillez utiliser un navigateur récent comme Chrome, Safari, Firefox ou Edge, ou choisir l'option application d'authentification à la place.
auth-passkey-setup-heading = Configurer une clé d'accès
auth-passkey-setup-description = Connectez-vous en toute sécurité avec Face ID, Touch ID, Windows Hello ou une clé de sécurité.
auth-passkey-setup-name-label = Nom de la clé d'accès
auth-passkey-setup-name-placeholder = par ex. MacBook Pro, iPhone
auth-passkey-setup-name-hint = Un nom pour identifier cette clé d'accès plus tard
auth-passkey-setup-create-button = Créer la clé d'accès
auth-passkey-setup-creating-button = Création de la clé d'accès...
auth-passkey-setup-device-iphone = iPhone
auth-passkey-setup-device-ipad = iPad
auth-passkey-setup-device-mac = Mac
auth-passkey-setup-device-windows = PC Windows
auth-passkey-setup-device-android = Appareil Android
auth-passkey-setup-device-linux = PC Linux
auth-passkey-setup-device-generic = Cet appareil
auth-passkey-setup-error-session-expired = Session expirée. Veuillez vous reconnecter.
auth-passkey-setup-error-cancelled = L'enregistrement a été annulé ou n'est pas autorisé
auth-passkey-setup-error-already-registered = Cette clé d'accès est déjà enregistrée
auth-passkey-setup-error-cancelled-generic = L'enregistrement a été annulé
auth-passkey-setup-error-generic = Échec de l'enregistrement de la clé d'accès
auth-passkey-setup-success-message = Clé d'accès créée avec succès
auth-passkey-setup-backup-codes-title = Enregistrez vos codes de récupération
auth-passkey-setup-backup-codes-description = Si vous perdez l'accès à votre clé d'accès, vous pouvez utiliser l'un de ces codes pour vous connecter. Chaque code ne peut être utilisé qu'une seule fois.
auth-passkey-setup-backup-codes-copy = Copier
auth-passkey-setup-backup-codes-copied = Copié !
auth-passkey-setup-backup-codes-download = Télécharger
auth-passkey-setup-backup-codes-acknowledge = J'ai enregistré mes codes de récupération
auth-passkey-setup-backup-file-title = Codes de récupération Nosdesk
auth-passkey-setup-backup-file-intro = Conservez ces codes dans un endroit sûr. Chaque code ne peut être utilisé qu'une seule fois.
auth-passkey-setup-success-heading = Clé d'accès créée !
auth-passkey-setup-success-description = Votre clé d'accès « { $name } » est prête à être utilisée.
auth-passkey-setup-success-protected-title = Votre compte est protégé
auth-passkey-setup-success-protected-description = La prochaine fois que vous vous connecterez, utilisez simplement votre empreinte, votre visage ou votre clé de sécurité au lieu d'un mot de passe.
auth-passkey-setup-success-cta = Commencer à utiliser Nosdesk !

# Paramètres : adresses e-mail (UserEmailsCard)
settings-emails-section-title = Adresses e-mail
settings-emails-add-button = Ajouter un e-mail
settings-emails-add-form-title = Ajouter une nouvelle adresse e-mail
settings-emails-add-placeholder = email@exemple.com
settings-emails-add-submit = Ajouter
settings-emails-add-submitting = Ajout...
settings-emails-add-cancel = Annuler
settings-emails-empty = Aucune adresse e-mail trouvée
settings-emails-primary-badge = Principale
settings-emails-verified-badge = Vérifiée
settings-emails-unverified-badge = Non vérifiée
settings-emails-type-personal = personnel
settings-emails-set-primary = Définir comme principale
settings-emails-remove = Supprimer
settings-emails-confirm-title = Supprimer l'adresse e-mail ?
settings-emails-confirm-message = { $email } ne sera plus associée à ce compte.
settings-emails-confirm-label = Supprimer
settings-emails-error-required = L'adresse e-mail est requise
settings-emails-error-invalid-format = Format d'e-mail non valide
settings-emails-add-success = Adresse e-mail ajoutée avec succès
settings-emails-add-error = Échec de l'ajout de l'adresse e-mail
settings-emails-set-primary-success = { $email } définie comme e-mail principal
settings-emails-set-primary-error = Échec de la définition de l'e-mail principal
settings-emails-delete-success = Adresse e-mail supprimée avec succès
settings-emails-delete-error = Échec de la suppression de l'adresse e-mail
# Docs : carte d'article (ArticleCard)
docs-article-card-updated = Mis à jour le { $date }
docs-article-card-edit = Modifier l'article

# Docs : gestion des collections (CollectionManager)
docs-collection-manager-title = Gérer les collections
docs-collection-manager-empty = Aucune collection disponible.
docs-collection-manager-pages = { $count ->
    [one] { $count } page
   *[other] { $count } pages
}
docs-collection-manager-system-badge = Système
docs-collection-manager-cancel = Annuler
docs-collection-manager-save = Enregistrer
docs-collection-manager-saving = Enregistrement...

# Docs : navigateur de collections (CollectionBrowser)
docs-collection-browser-heading = Collections
docs-collection-browser-new = Nouveau
docs-collection-browser-name-placeholder = Nom de la collection...
docs-collection-browser-cancel = Annuler
docs-collection-browser-create = Créer
docs-collection-browser-loading-label = Chargement des collections
docs-collection-browser-pages = { $count ->
    [one] { $count } page
   *[other] { $count } pages
}
docs-collection-browser-system-badge = Système
docs-collection-browser-restricted-badge = Restreinte
docs-collection-browser-empty = Aucune collection pour le moment.

# Docs : élément d'arborescence (CollectionTreeItem)
docs-collection-tree-item-untitled = Sans titre
docs-collection-tree-item-draft = Brouillon
docs-collection-tree-item-override-title = Permissions personnalisées

# Docs : arborescence (CollectionTreeList)
docs-collection-tree-list-empty = Aucune page dans cette collection pour le moment.

# Docs : visibilité de la collection (CollectionVisibilityModal)
docs-collection-visibility-title = Accès à la collection
docs-collection-visibility-description = Sélectionnez les groupes et utilisateurs autorisés à accéder à cette collection. Sans sélection, la collection est publique (visible par tous).
docs-collection-visibility-public = Publique, visible par tous les utilisateurs
docs-collection-visibility-picker-placeholder = Rechercher des utilisateurs et groupes...
docs-collection-visibility-cancel = Annuler
docs-collection-visibility-save = Enregistrer
docs-collection-visibility-saving = Enregistrement...

# Docs : menu d'actions (DocumentActionsMenu)
docs-actions-menu-subscribe = S'abonner
docs-actions-menu-unsubscribe = Se désabonner
docs-actions-menu-insights = Statistiques
docs-actions-menu-history = Historique des révisions
docs-actions-menu-print = Imprimer
docs-actions-menu-duplicate = Dupliquer
docs-actions-menu-export = Télécharger en Markdown
docs-actions-menu-move = Déplacer vers...
docs-actions-menu-collections = Collections
docs-actions-menu-archive = Archiver
docs-actions-menu-unarchive = Désarchiver
docs-actions-menu-permissions = Permissions
docs-actions-menu-publish = Publier
docs-actions-menu-unpublish = Dépublier
docs-actions-menu-trash = Mettre à la corbeille
docs-actions-menu-trash-confirm = Confirmer la mise à la corbeille ?
docs-actions-menu-trigger = Actions de la page

# Docs : fil d'Ariane (DocumentationBreadcrumb)
docs-breadcrumb-root = Documentation
docs-breadcrumb-aria = Fil d'Ariane

# Docs : carte (DocumentationCard)
docs-card-empty-content = Aucun contenu pour le moment
docs-card-children-more = +{ $count } de plus
docs-card-relative-unknown = Inconnu
docs-card-relative-today = Aujourd'hui
docs-card-relative-yesterday = Hier
docs-card-relative-days = il y a { $count }j
docs-card-relative-weeks = il y a { $count }sem.
docs-card-freshness-fresh = Mise à jour récente
docs-card-freshness-recent = Mise à jour cette semaine
docs-card-freshness-stale = Pas de mise à jour récente

# Docs : squelette de carte (DocumentationCardSkeleton)
docs-card-skeleton-label = Chargement des pages

# Docs : squelette de ligne (DocumentationRowSkeleton)
docs-row-skeleton-label = Chargement des pages

# Docs : navigation (DocumentationNav)
docs-nav-starred = Favoris
docs-nav-empty = Aucun document pour le moment
docs-nav-sort-manual = Manuel
docs-nav-sort-alpha = Alphabétique
docs-nav-sort-recent = Récemment mis à jour
docs-nav-untitled = Sans titre
docs-nav-duplicate-suffix = { $title } (copie)
docs-nav-confirm-delete-collection-title = Supprimer { $name } ?
docs-nav-confirm-delete-collection-fallback = Supprimer la collection ?
docs-nav-confirm-delete-collection-message = Les pages de cette collection seront déplacées vers la corbeille. Vous pouvez les restaurer depuis là.
docs-nav-confirm-delete = Supprimer
docs-nav-menu-open-new-tab = Ouvrir dans un nouvel onglet
docs-nav-menu-copy-link = Copier le lien
docs-nav-menu-copy-md = Copier en Markdown
docs-nav-menu-copy-text = Copier en texte brut
docs-nav-menu-add-child = Ajouter une page enfant
docs-nav-menu-star = Mettre en favori
docs-nav-menu-unstar = Retirer des favoris
docs-nav-menu-subscribe = S'abonner
docs-nav-menu-duplicate = Dupliquer
docs-nav-menu-move = Déplacer vers...
docs-nav-menu-history = Historique des révisions
docs-nav-menu-insights = Statistiques
docs-nav-menu-export-md = Télécharger en Markdown
docs-nav-menu-print = Imprimer
docs-nav-menu-permissions = Permissions
docs-nav-menu-archive = Archiver
docs-nav-menu-restore = Restaurer
docs-nav-menu-trash = Mettre à la corbeille
docs-nav-col-edit = Modifier la collection
docs-nav-col-sort-heading = Trier par
docs-nav-col-permissions = Permissions
docs-nav-col-delete = Supprimer

# Docs : actions de ligne (NavRowActions)
docs-nav-row-more = Plus d'actions pour { $label }
docs-nav-row-add = Ajouter une nouvelle page à { $label }

# Docs : élément de navigation (DocumentationNavItem)
docs-nav-item-draft = Brouillon

# Docs : élément d'arborescence (DocumentationTreeItem)
docs-tree-item-expand = Développer
docs-tree-item-collapse = Réduire

# Docs : élément de sommaire (DocumentationTocItem)
docs-toc-item-untitled = Page sans titre

# Docs : panneau d'aperçu (DocumentInsightsPanel)
docs-insights-title = Aperçu
docs-insights-source-heading = Source
docs-insights-stats-heading = Statistiques
docs-insights-contributors-heading = Contributeurs
docs-insights-created = Créée { $relative }
docs-insights-updated = Mise à jour { $relative }
docs-insights-reading-time = { $minutes ->
    [one] { $minutes } minute de lecture
   *[other] { $minutes } minutes de lecture
}
docs-insights-word-count = { $count } mots
docs-insights-char-count = { $count } caractères
docs-insights-emoji-count = { $count } emoji
docs-insights-contributors-loading = Chargement des contributeurs...
docs-insights-contributors-empty = Aucun contributeur pour le moment.
docs-insights-contributor-role = Contributeur
docs-insights-unknown-user = Utilisateur inconnu
docs-insights-relative-unknown = inconnu
docs-insights-relative-just-now = à l'instant
docs-insights-relative-minutes = il y a { $count } min
docs-insights-relative-hours = il y a { $count } h
docs-insights-relative-days = { $count ->
    [one] il y a { $count } jour
   *[other] il y a { $count } jours
}
docs-insights-relative-months = { $count ->
    [one] il y a { $count } mois
   *[other] il y a { $count } mois
}
docs-insights-relative-years = { $count ->
    [one] il y a { $count } an
   *[other] il y a { $count } ans
}

# Docs : édition de collection (EditCollectionModal)
docs-edit-collection-title = Modifier la collection
docs-edit-collection-name = Nom
docs-edit-collection-slug = Identifiant URL
docs-edit-collection-slug-help = Fragment d'URL pour cette collection. Minuscules, chiffres et tirets uniquement.
docs-edit-collection-icon = Icône
docs-edit-collection-color = Couleur
docs-edit-collection-description = Description courte
docs-edit-collection-description-placeholder = Slogan facultatif affiché au-dessus de l'aperçu de la collection
docs-edit-collection-description-help = L'aperçu complet se modifie directement sur la page d'accueil de la collection.
docs-edit-collection-hide-titles-aria = Masquer les titres de page aux non-membres
docs-edit-collection-hide-titles-label = Masquer les titres de page aux non-membres
docs-edit-collection-hide-titles-help = Les wikilinks inter-collections affichent « Page restreinte » pour les lecteurs sans accès, au lieu de divulguer le titre. Recommandé pour les collections sensibles.
docs-edit-collection-name-required = Le nom est requis.
docs-edit-collection-save-error = Échec de l'enregistrement. Réessayez.
docs-edit-collection-cancel = Annuler
docs-edit-collection-save = Enregistrer les modifications
docs-edit-collection-saving = Enregistrement...

# Docs : déplacement de document (MoveDocumentModal)
docs-move-title = Déplacer le document
docs-move-search-placeholder = Rechercher des pages...
docs-move-root-label = Niveau racine (sans parent)
docs-move-current-badge = Actuel
docs-move-empty-search = Aucune page correspondante.
docs-move-empty = Aucune page disponible.
docs-move-cancel = Annuler
docs-move-action = Déplacer
docs-move-moving = Déplacement...

# Docs : permissions de page (PagePermissionsModal)
docs-page-permissions-title = Permissions de la page
docs-page-permissions-mode-inherit = Hériter des collections
docs-page-permissions-mode-custom = Accès personnalisé
docs-page-permissions-inherit-description = Cette page hérite de la visibilité de ses collections. Les utilisateurs ayant accès à l'une des collections de la page peuvent voir cette page.
docs-page-permissions-no-collections = Dans aucune collection, visible par tous.
docs-page-permissions-custom-description = Sélectionnez les groupes et utilisateurs autorisés à accéder à cette page. Cela remplace les permissions au niveau collection.
docs-page-permissions-picker-placeholder = Rechercher des utilisateurs et groupes...
docs-page-permissions-no-selection-warning = Aucun groupe ni utilisateur sélectionné, seuls les administrateurs pourront voir cette page.
docs-page-permissions-cancel = Annuler
docs-page-permissions-save = Enregistrer
docs-page-permissions-saving = Enregistrement...

# Docs : tickets liés (PageTicketLinksPanel)
docs-page-tickets-heading = Tickets liés
docs-page-tickets-add = Lier un ticket
docs-page-tickets-loading = Chargement...
docs-page-tickets-empty = Aucun ticket lié à cette page pour le moment.
docs-page-tickets-resolved-heading = Résolus
docs-page-tickets-referenced-heading = Référencés
docs-page-tickets-fallback-title = Ticket nº{ $id }
docs-page-tickets-unlink = Délier le ticket nº{ $id }

# Docs : badge d'auteur (DocumentAuthorBadge)
docs-author-badge-fallback-name = Inconnu
docs-author-badge-verifier-fallback = Quelqu'un
docs-author-badge-title-verified = Rédigé par { $author } · vérifié { $relative }
docs-author-badge-title-basic = Rédigé par { $author }
docs-author-badge-popover-aria = Auteur et vérification du document
docs-author-badge-created = Créé
docs-author-badge-author = Auteur
docs-author-badge-last-edited-by = Dernière modification par
docs-author-badge-verification = Vérification
docs-author-badge-state-verified = Vérifié
docs-author-badge-state-stale = Obsolète
docs-author-badge-state-never = Non vérifié
docs-author-badge-last-verified = Dernière vérification
docs-author-badge-verify-prompt-never = Marquer comme vérifié, revérifier tous les :
docs-author-badge-verify-prompt-again = Revérifier tous les :
docs-author-badge-interval-30d = 30 j
docs-author-badge-interval-90d = 90 j
docs-author-badge-interval-180d = 180 j
docs-author-badge-interval-1y = 1 an
docs-author-badge-interval-never = Jamais
docs-author-badge-clear = Effacer la vérification
# Ticket : champs latéraux du détail & en-tête d'impression (TicketDetails).
ticket-detail-title-label = Titre
ticket-detail-source-label = Source
ticket-detail-source-tooltip = Ouvert via { $provider }. Les réponses sont relayées dans le fil.
ticket-detail-source-email = E-mail
ticket-detail-source-slack = Slack
ticket-detail-source-teams = Microsoft Teams
ticket-detail-clear-requester = Effacer le demandeur
ticket-detail-add-requester = Ajouter un demandeur
ticket-detail-find-user-placeholder = Rechercher un utilisateur...
ticket-detail-assign-to-placeholder = Attribuer à...
ticket-detail-clear-assignee = Effacer l'attribution
ticket-detail-add-assignee = Ajouter un destinataire
ticket-detail-claim = Prendre
ticket-detail-claim-title = M'attribuer ce ticket
ticket-detail-sla-label = SLA
ticket-detail-scheduling-label = Planification
ticket-detail-scheduling-none = Aucune
ticket-detail-scheduling-due-date = Échéance
ticket-detail-scheduling-due-prefix = Échéance { $date }
ticket-detail-scheduling-clear-due = Effacer l'échéance
ticket-detail-scheduling-recurrence = Récurrence
ticket-detail-recurrence-none = Non récurrent
ticket-detail-recurrence-daily = Quotidien
ticket-detail-recurrence-weekly = Hebdomadaire
ticket-detail-recurrence-weekdays = Jours ouvrés
ticket-detail-recurrence-monthly = Mensuel
ticket-detail-recurrence-yearly = Annuel
ticket-detail-recurrence-recurring = Récurrent
ticket-detail-recurrence-custom-note = Règle RRULE personnalisée ({ $rule }). Modifiez via l'API.
ticket-detail-recurrence-respawn-note = La clôture de ce ticket génère la prochaine occurrence.
ticket-detail-category-placeholder = Sélectionner une catégorie...
ticket-detail-cycle-label = Cycle
ticket-detail-cycle-tooltip = Cycle { $name } ({ $state })
ticket-detail-resolution-label = Résolution
ticket-detail-resolution-closed = Fermé
ticket-detail-resolution-draft-from-notes = Brouillon depuis les notes
ticket-detail-resolution-draft-from-notes-title = { $count ->
    [one] Ajouter { $count } note interne au brouillon de résolution
   *[other] Ajouter { $count } notes internes au brouillon de résolution
  }
ticket-detail-resolution-placeholder-active = Notez ce qui a corrigé le problème : la réponse pour le prochain intervenant.
ticket-detail-resolution-placeholder-draft = Vous pouvez rédiger ici vos notes sur la résolution pendant le traitement.
ticket-detail-audit-created = Créé
ticket-detail-audit-modified = Dernière modification
ticket-detail-audit-closed = Fermé
ticket-detail-print-status = Statut
ticket-detail-print-priority = Priorité
ticket-detail-print-category = Catégorie
ticket-detail-print-requester = Demandeur
ticket-detail-print-assignee = Attribué à
ticket-detail-print-created = Créé
ticket-detail-print-modified = Modifié
ticket-detail-print-unassigned = Non attribué
ticket-detail-print-unknown = Inconnu
ticket-detail-print-logo-alt = Logo
ticket-detail-print-qr-alt = QR Code du ticket
ticket-detail-print-qr-label = Scannez pour ouvrir

# Ticket : commentaires & pièces jointes (CommentsAndAttachments).
ticket-comments-section-title = Commentaires et pièces jointes
ticket-comments-drop-files = Déposez les fichiers ici
ticket-comments-internal-banner = Visible par l'équipe uniquement. Non transmis via le canal du ticket.
ticket-comments-placeholder-public = Ajouter un nouveau commentaire...
ticket-comments-placeholder-internal = Note pour l'équipe…
ticket-comments-record-voice = Enregistrer une note vocale
ticket-comments-upload-file = Téléverser un fichier
ticket-comments-visibility-group = Visibilité de la réponse
ticket-comments-public-reply = Réponse publique
ticket-comments-public-reply-title = Envoyé au demandeur via le canal du ticket
ticket-comments-internal-note = Note interne
ticket-comments-internal-note-title = Visible uniquement par les techniciens ; jamais transmis via le canal du ticket
ticket-comments-submit-reply = Envoyer la réponse
ticket-comments-submit-note = Ajouter la note
ticket-comments-voice-note-filename = Note vocale { $date }
ticket-comments-filter-group = Filtre de visibilité des commentaires
ticket-comments-filter-all = Tous ({ $count })
ticket-comments-filter-public = Publics ({ $count })
ticket-comments-filter-internal = Internes ({ $count })
ticket-comments-badge-internal = Interne
ticket-comments-badge-forwarded = Transféré
ticket-comments-badge-forwarded-title = Un technicien a transféré cet e-mail dans le helpdesk
ticket-comments-action-download = Télécharger
ticket-comments-action-delete-comment = Supprimer le commentaire
ticket-comments-action-delete-voice = Supprimer le message vocal
ticket-comments-audio-default = Audio
ticket-comments-audio-voice-message = Message vocal
ticket-comments-print-unknown-author = Inconnu
ticket-comments-show-quoted-thread = Afficher le fil cité
ticket-comments-show-quoted-reply = { $lines ->
    [one] Afficher la réponse citée ({ $lines } ligne)
   *[other] Afficher la réponse citée ({ $lines } lignes)
  }
ticket-comments-show-original = Afficher le message d'origine
ticket-comments-show-original-title = Ouvrir la source RFC-822 brute dans un nouvel onglet

# Ticket : journal d'activité (TicketActivity).
ticket-activity-section-title = Activité
ticket-activity-load-error = Échec du chargement de l'activité
ticket-activity-load-more-error = Échec du chargement d'activité supplémentaire
ticket-activity-empty = Aucune activité pour l'instant.
ticket-activity-load-more = Charger l'historique plus ancien
ticket-activity-loading = Chargement…
ticket-activity-actor-someone = Quelqu'un
ticket-activity-actor-system = Système
ticket-activity-actor-sender = Expéditeur
ticket-activity-actor-email-aria = Expéditeur de l'e-mail
ticket-activity-actor-portal-aria = Soumission via le portail public
ticket-activity-actor-portal-label = le portail public
ticket-activity-channel-email = e-mail
ticket-activity-channel-slack = Slack
ticket-activity-channel-teams = Microsoft Teams
ticket-activity-channel-discord = Discord
ticket-activity-actor-title-subject = { $name } — Objet : { $subject }
ticket-activity-actor-title-named = { $name } <{ $email }>
ticket-activity-actor-title-named-subject = { $name } <{ $email }> — Objet : { $subject }
ticket-activity-to-assignee = à { $name }
ticket-activity-phrase-created = a créé ce ticket
ticket-activity-phrase-opened-via = a ouvert ce ticket via { $channel }
ticket-activity-phrase-submitted-via = a soumis ce ticket via { $channel }
ticket-activity-phrase-deleted = a supprimé ce ticket
ticket-activity-phrase-status-set = a défini le statut sur { $name }
ticket-activity-phrase-status-changed = a changé le statut
ticket-activity-phrase-reassigned = a réattribué ce ticket
ticket-activity-phrase-unassigned = a retiré l'attribution de ce ticket
ticket-activity-phrase-priority-set = a défini la priorité sur { $priority }
ticket-activity-phrase-priority-changed = a changé la priorité
ticket-activity-phrase-renamed = a renommé le ticket en « { $title } »
ticket-activity-phrase-renamed-plain = a renommé le ticket
ticket-activity-phrase-category-changed = a changé la catégorie
ticket-activity-phrase-verification-changed = a mis à jour l'état de vérification
ticket-activity-phrase-tags-added = { $count ->
    [one] a ajouté une étiquette
   *[other] a ajouté { $count } étiquettes
  }
ticket-activity-phrase-tags-removed = { $count ->
    [one] a retiré une étiquette
   *[other] a retiré { $count } étiquettes
  }
ticket-activity-phrase-tags-updated = a mis à jour les étiquettes
ticket-activity-phrase-resolution-changed = a mis à jour les notes de résolution
ticket-activity-phrase-watcher-self-start = a commencé à suivre ce ticket
ticket-activity-phrase-watcher-self-auto = a commencé à suivre (abonnement automatique à la première réponse)
ticket-activity-phrase-watcher-self-stop = a arrêté de suivre ce ticket
ticket-activity-phrase-watcher-added-named = a ajouté { $name } comme observateur
ticket-activity-phrase-watcher-added = a ajouté un observateur
ticket-activity-phrase-watcher-removed-named = a retiré { $name } des observateurs
ticket-activity-phrase-watcher-removed = a retiré un observateur
ticket-activity-phrase-updated = a mis à jour le ticket
ticket-activity-phrase-internal-note = a ajouté une note interne
ticket-activity-phrase-replied-via = a répondu via { $channel }
ticket-activity-phrase-comment-via = a ajouté un commentaire via { $channel }
ticket-activity-phrase-commented = a commenté ce ticket
ticket-activity-phrase-comment-deleted = a supprimé un commentaire
ticket-activity-phrase-generic = a effectué une modification

# Ticket : sélecteur d'étiquettes (TicketTagsField).
ticket-field-tags-label = Étiquettes
ticket-field-tags-add = Ajouter une étiquette
ticket-field-tags-remove = Retirer { $name }
ticket-field-tags-picker-placeholder = Rechercher ou créer une étiquette…
ticket-field-tags-loading = Chargement…
ticket-field-tags-no-match = Aucune étiquette correspondante.
ticket-field-tags-create = Créer « { $name } »
ticket-field-tags-creating = Création…
ticket-field-tags-done = Terminé

# Ticket : observateurs (TicketWatchersField).
ticket-field-watchers-label = Observateurs
ticket-field-watchers-watching = Suivi
ticket-field-watchers-watch = Suivre
ticket-field-watchers-watch-title = Suivre ce ticket pour recevoir les mises à jour
ticket-field-watchers-unwatch-title = Arrêter de suivre ce ticket
ticket-field-watchers-notify-internal = Notifier sur les notes internes
ticket-field-watchers-public-only = Réponses publiques uniquement
ticket-field-watchers-toggle-on = ACTIVÉ
ticket-field-watchers-toggle-off = DÉSACTIVÉ
ticket-field-watchers-pref-load-error = Échec du chargement de la préférence
ticket-field-watchers-pref-save-error = Échec de l'enregistrement de la préférence
ticket-field-watchers-overflow-title = { $count ->
    [one] { $count } de plus
   *[other] { $count } de plus
  }

# Ticket : ligne d'appareils (TicketDevicesField).
ticket-field-devices-label = Appareils
ticket-field-devices-add = Ajouter un appareil
ticket-field-devices-detach = Détacher l'appareil
ticket-field-devices-fallback-name = Appareil n°{ $id }
ticket-field-devices-title-with-model = { $hostname } · { $model }

# Ticket : tickets liés (TicketLinkedTicketsField).
ticket-field-linked-tickets-label = Tickets liés
ticket-field-linked-tickets-add = Lier un ticket
ticket-field-linked-tickets-drop = Déposer pour lier

# Ticket : projets (TicketProjectsField).
ticket-field-projects-label = Projets
ticket-field-projects-add = Ajouter à un projet
ticket-field-projects-remove = Retirer du projet
ticket-field-projects-fallback = Projet n°{ $id }

# Ticket : documentation liée (TicketLinkedDocs).
ticket-field-docs-label = Documentation
ticket-field-docs-add = Enregistrer comme doc
ticket-field-docs-resolves-title = { $title } · résout ce ticket

# Ticket : puces / badges.
ticket-chip-remove = Retirer { $label }
ticket-chip-sidebar-remove = Retirer
ticket-chip-linked-ticket-fallback = Ticket n°{ $id }
ticket-chip-linked-ticket-title = n°{ $id } · { $title }
ticket-chip-unlink-ticket = Délier le ticket
ticket-chip-gap-flagged = Signalé pour documentation
ticket-chip-gap-view-queue = Voir la file →
ticket-chip-gap-remove-flag = Retirer le signalement
ticket-chip-preview-priority = Priorité
ticket-chip-preview-created = Créé
ticket-chip-preview-requester = Demandeur
ticket-chip-preview-assignee = Attribué à
ticket-chip-preview-unassigned = Non attribué
ticket-chip-preview-unlink = Délier le ticket
ticket-chip-device-warranty-active = Active
ticket-chip-device-warranty-warning = Avertissement
ticket-chip-device-warranty-expired = Expirée
ticket-chip-device-remove = Retirer l'appareil
ticket-chip-device-view-title = Voir l'appareil
ticket-chip-device-unnamed = Appareil sans nom
ticket-chip-device-field-serial = N° de série
ticket-chip-device-field-model = Modèle
ticket-chip-device-field-manufacturer = Fabricant
ticket-chip-device-field-hostname = Nom d'hôte
ticket-chip-device-value-na = N/D
ticket-chip-device-value-unknown = Inconnu
ticket-chip-device-copy-tooltip = Cliquez pour copier
ticket-chip-device-copied = Copié !

# Ticket : sélecteur statut/priorité/catégorie (CustomDropdown).
ticket-chip-dropdown-select = Sélectionner...
ticket-chip-dropdown-status = Sélectionner un statut
ticket-chip-dropdown-priority = Sélectionner une priorité
ticket-chip-dropdown-category = Sélectionner une catégorie
ticket-chip-dropdown-option = Sélectionner une option
# Admin : notice de configuration par variables d'environnement
admin-env-notice-title = Configuration par variables d'environnement
admin-env-notice-prefix = Les paramètres sont configurés via des variables d'environnement dans votre fichier
admin-env-notice-suffix = ou l'environnement Docker.

# Admin : carte d'informations système (SystemInfoCard)
admin-system-info-title = Informations système
admin-system-info-version = Version
admin-system-info-environment = Environnement
admin-system-info-uptime = Disponibilité
admin-system-info-update-to = Mettre à jour vers { $version }
admin-system-info-uptime-days = { $count } j
admin-system-info-uptime-hours = { $count } h
admin-system-info-uptime-minutes = { $count } min
admin-system-info-uptime-seconds = { $count } s

# Admin : éditeur de catégories (CategoryEditPanel)
admin-categories-edit-title-edit = Modifier la catégorie
admin-categories-edit-title-create = Créer une catégorie
admin-categories-edit-delete-tooltip = Supprimer la catégorie
admin-categories-edit-close-tooltip = Fermer le panneau
admin-categories-edit-name-label = Nom
admin-categories-edit-name-placeholder = Saisir le nom de la catégorie
admin-categories-edit-description-label = Description
admin-categories-edit-description-placeholder = Description facultative
admin-categories-edit-icon-label = Icône
admin-categories-edit-icon-folder = Dossier
admin-categories-edit-icon-tag = Étiquette
admin-categories-edit-icon-bug = Anomalie
admin-categories-edit-icon-settings = Paramètres
admin-categories-edit-icon-idea = Idée
admin-categories-edit-icon-question = Question
admin-categories-edit-icon-alert = Alerte
admin-categories-edit-icon-star = Étoile
admin-categories-edit-color-label = Couleur
admin-categories-edit-active-label = Actif
admin-categories-edit-visibility-label = Visible aux groupes
admin-categories-edit-visibility-hint = (laisser vide pour public)
admin-categories-edit-visibility-toggle-aria = Basculer la visibilité pour { $name }
admin-categories-edit-member-count = { $count ->
    [one] { $count } membre
   *[other] { $count } membres
    }
admin-categories-edit-no-groups = Aucun groupe disponible. Créez d'abord des groupes.
admin-categories-edit-cancel = Annuler
admin-categories-edit-save = Enregistrer les modifications
admin-categories-edit-create = Créer la catégorie

# Admin : configuration des groupes (GroupConfigurationPanel)
admin-groups-config-subtitle = Configuration du groupe
admin-groups-config-delete-tooltip = Supprimer le groupe
admin-groups-config-close-tooltip = Fermer le panneau
admin-groups-config-source-microsoft = Microsoft Entra ID
admin-groups-config-managed-by = Géré par { $source }
admin-groups-config-last-synced = Dernière synchronisation le { $date }
admin-groups-config-unmanage = Reprendre la gestion
admin-groups-config-unmanage-processing = Traitement...
admin-groups-config-sync-settings = Paramètres de synchronisation
admin-groups-config-general = Informations générales
admin-groups-config-name-label = Nom
admin-groups-config-name-placeholder = Saisir le nom du groupe
admin-groups-config-description-label = Description
admin-groups-config-description-placeholder = Description facultative
admin-groups-config-color-label = Couleur
admin-groups-config-save-changes = Enregistrer les modifications
admin-groups-config-members = Membres
admin-groups-config-no-members = Aucun membre
admin-groups-config-devices = Appareils
admin-groups-config-device-sn = N° de série : { $sn }
admin-groups-config-no-devices = Aucun appareil
admin-groups-config-included-in = Inclus dans
admin-groups-config-included-groups = Groupes inclus
admin-groups-config-includes-hint = Les membres des groupes inclus sont considérés comme membres de ce groupe pour la visibilité, l'accès et l'attribution.
admin-groups-config-source-direct = Direct
admin-groups-config-source-via = via
admin-groups-config-source-also-via = également via
admin-groups-config-section-assigned = Affectés
admin-groups-config-section-included-via = Inclus via les groupes
admin-groups-config-section-not-assigned = Non affectés
admin-groups-config-search-users = Rechercher des utilisateurs...
admin-groups-config-search-devices = Rechercher des appareils par nom, hôte, numéro de série...
admin-groups-config-search-groups = Rechercher des groupes...
admin-groups-config-no-users-found = Aucun utilisateur trouvé
admin-groups-config-no-devices-found = Aucun appareil trouvé
admin-groups-config-no-groups-found = Aucun groupe trouvé
admin-groups-config-synced-badge = Synchronisé
admin-groups-config-synced-intune-tooltip = Synchronisé depuis Microsoft Intune
admin-groups-config-selected-count = { $count } sélectionné(s)
admin-groups-config-member-count = { $count ->
    [one] { $count } membre
   *[other] { $count } membres
    }
admin-groups-config-save-members = Enregistrer les membres
admin-groups-config-save-devices = Enregistrer les appareils
admin-groups-config-save-includes = Enregistrer les groupes inclus
admin-groups-config-not-found = Groupe introuvable
admin-groups-config-cancel = Annuler
admin-groups-config-delete-title = Supprimer le groupe
admin-groups-config-delete-confirm = Supprimer le groupe
admin-groups-config-delete-prompt-prefix = Êtes-vous sûr de vouloir supprimer le groupe
admin-groups-config-delete-prompt-suffix = ? Toutes les associations de membres seront supprimées, mais pas les utilisateurs.
admin-groups-config-unmanage-title = Reprendre la gestion du groupe ?
admin-groups-config-unmanage-title-named = Reprendre la gestion de { $name } ?
admin-groups-config-unmanage-message = Le groupe ne sera plus synchronisé avec Microsoft Entra ID. Les modifications manuelles seront autorisées, mais l'historique de synchronisation est conservé.
admin-groups-config-error-invalid-id = Identifiant de groupe invalide
admin-groups-config-error-load = Échec du chargement des détails du groupe
admin-groups-config-error-name-required = Le nom du groupe est requis
admin-groups-config-error-save = Échec de l'enregistrement du groupe
admin-groups-config-error-members = Échec de la mise à jour des membres
admin-groups-config-error-devices = Échec de la mise à jour des appareils
admin-groups-config-error-includes = Échec de la mise à jour des groupes inclus
admin-groups-config-error-delete = Échec de la suppression du groupe
admin-groups-config-error-unmanage = Échec de la reprise de gestion
admin-groups-config-success-updated = Groupe mis à jour
admin-groups-config-success-members = Membres mis à jour
admin-groups-config-success-devices = Appareils mis à jour
admin-groups-config-success-includes = Groupes inclus mis à jour
admin-groups-config-success-unmanage = Le groupe est désormais géré localement

# Chrome de la table des tickets (TicketsTable, TicketRow, TicketPreviewPane)
views-tickets-table-select-all-aria = Sélectionner tous les tickets visibles
views-tickets-table-resize-handle-tooltip = Glisser pour redimensionner · double-clic pour ajuster
views-ticket-row-select-aria = Sélectionner le ticket n°{ $id }
views-ticket-row-recurring-tooltip = Ticket récurrent
views-ticket-row-sla-badge = SLA
views-ticket-row-sla-breached-tooltip = SLA dépassé
views-ticket-row-sla-breached = Dépassé
views-ticket-row-sla-paused = En pause
views-ticket-row-sla-on-track = Dans les temps
views-ticket-row-cycle-tooltip = Appartient à un cycle
views-ticket-row-cycle-label = cycle n°{ $id }
views-ticket-row-no-due-date = Aucune échéance
views-ticket-row-kb-badge = KB
views-ticket-row-kb-gap-tooltip = Signal de lacune de connaissance : { $signal }
views-ticket-row-devices-count = { $count ->
    [one] { $count } appareil
   *[other] { $count } appareils
    }

# Volet d'aperçu du ticket (TicketPreviewPane)
views-ticket-preview-aria = Aperçu du ticket
views-ticket-preview-empty-title = Aucun ticket sélectionné
views-ticket-preview-empty-prefix = Cliquez sur une ligne, ou parcourez avec
views-ticket-preview-empty-suffix = pour afficher l'aperçu.
views-ticket-preview-open = Ouvrir
views-ticket-preview-close-tooltip = Fermer l'aperçu (Échap)
views-ticket-preview-close-aria = Fermer l'aperçu
views-ticket-preview-kb-gap = Lacune KB
views-ticket-preview-recurring = Récurrent
views-ticket-preview-properties = Propriétés
views-ticket-preview-assignee = Assigné à
views-ticket-preview-requester = Demandeur
views-ticket-preview-due-date = Échéance
views-ticket-preview-not-set = Non définie
views-ticket-preview-cycle = Cycle
views-ticket-preview-cycle-label = Cycle n°{ $id }
views-ticket-preview-category = Catégorie
views-ticket-preview-sla = SLA
views-ticket-preview-activity = Activité
views-ticket-preview-last-activity = Dernière activité
views-ticket-preview-created = Créé
views-ticket-preview-affected-devices = Appareils concernés
views-ticket-preview-more-devices = { $count ->
    [one] +{ $count } autre
   *[other] +{ $count } autres
    }
views-ticket-preview-view-full = Voir description, commentaires et appareils

# Carte thermique des tickets (TicketHeatmap)
ticket-heatmap-title-closed = Tickets clôturés
ticket-heatmap-title-activity = Activité des tickets
ticket-heatmap-error-load = Échec du chargement des données. Veuillez réessayer.
ticket-heatmap-tooltip-empty = Aucun ticket
ticket-heatmap-tooltip-count = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
    }
ticket-heatmap-day-sun = Dim
ticket-heatmap-day-mon = Lun
ticket-heatmap-day-tue = Mar
ticket-heatmap-day-wed = Mer
ticket-heatmap-day-thu = Jeu
ticket-heatmap-day-fri = Ven
ticket-heatmap-day-sat = Sam
ticket-heatmap-days-with-activity = { $count ->
    [one] { $count } jour avec activité
   *[other] { $count } jours avec activité
    }
ticket-heatmap-legend-less = Moins
ticket-heatmap-legend-more = Plus

# Vues : menu d'ajout de filtre (AddFilterMenu)
views-add-filter-trigger = Ajouter un filtre
views-add-filter-back-tooltip = Retour (Retour arrière)
views-add-filter-search-title-placeholder = Rechercher un titre…
views-add-filter-no-matches = Aucune valeur correspondante
views-add-filter-facet-title = Titre
views-add-filter-facet-status = Statut
views-add-filter-facet-priority = Priorité
views-add-filter-facet-assignee = Assigné à
views-add-filter-facet-sla = SLA
views-add-filter-facet-cycle = Cycle

# Vues : liste de valeurs de filtre (FilterValueList)
views-filter-value-search-placeholder = Rechercher…
views-filter-value-no-matches = Aucun résultat
views-filter-value-no-options = Aucune option
views-filter-value-clear = Effacer

# Vues : menu d'affichage (DisplayMenu)
views-display-menu-trigger = Affichage
views-display-menu-trigger-tooltip = Options d'affichage
views-display-menu-grouping = Regroupement
views-display-menu-density = Densité
views-display-menu-density-aria = Densité des lignes
views-display-menu-density-compact = Compact
views-display-menu-density-cosy = Confortable
views-display-menu-density-comfortable = Spacieux
views-display-menu-group-none = Aucun
views-display-menu-group-status = Statut
views-display-menu-group-priority = Priorité
views-display-menu-group-assignee = Assigné
views-display-menu-group-sla = SLA
views-display-menu-group-cycle = Cycle
views-display-menu-properties = Propriétés
views-display-menu-column-ticket-id = N° ticket
views-display-menu-reset = Réinitialiser les colonnes
views-display-menu-reset-tooltip = Restaurer l'ordre, la largeur et la visibilité par défaut
views-display-menu-save-to-view = Enregistrer dans la vue

# Vues : barres d'onglets
views-tab-bar-aria = Vue
views-project-tab-aria = Vue du projet
views-project-tab-board = Tableau
views-project-tab-gantt = Gantt
views-project-tab-cycles = Cycles

# Vues : modale d'édition de vue enregistrée (SavedViewEditorModal)
views-saved-editor-title = Modifier la vue
views-saved-editor-name-label = Nom
views-saved-editor-delete = Supprimer la vue
views-saved-editor-cancel = Annuler
views-saved-editor-save = Enregistrer
views-saved-editor-saving = Enregistrement
views-saved-editor-confirm-title = Supprimer la vue ?
views-saved-editor-confirm-message = Supprimer « { $name } » ? Action irréversible, recréez la vue si besoin.

# Vues : pastille de filtre (FilterPill)
views-filter-pill-remove-tooltip = Supprimer le filtre { $label }
views-filter-pill-search-title-placeholder = Rechercher un titre…

# Vues : sélecteur de vue (ViewSwitcher)
views-view-switcher-placeholder = Vue
views-view-switcher-edit-view = Modifier la vue…

# Tableau de bord : widget tickets assignés (UserAssignedTickets)
user-assigned-tickets-title-assigned = Tickets assignés
user-assigned-tickets-title-requested = Tickets demandés
user-assigned-tickets-empty-title-assigned = Aucun ticket assigné
user-assigned-tickets-empty-title-requested = Aucun ticket demandé
user-assigned-tickets-empty-current = Tout est à jour !
user-assigned-tickets-error-assigned = Échec du chargement des tickets assignés
user-assigned-tickets-error-requested = Échec du chargement des tickets demandés
user-assigned-tickets-status-active = Actifs
user-assigned-tickets-status-active-desc = Ouverts + En cours
user-assigned-tickets-status-open = Ouverts
user-assigned-tickets-status-in-progress = En cours
user-assigned-tickets-status-closed = Clôturés
user-assigned-tickets-status-all = Tous
user-assigned-tickets-status-all-desc = Tous les statuts
user-assigned-tickets-status-filter-aria = Filtre de statut { $title }
user-assigned-tickets-sort-priority = Priorité
user-assigned-tickets-sort-priority-desc = Priorité, puis récents
user-assigned-tickets-sort-recent = Récents
user-assigned-tickets-sort-recent-desc = Modifiés en dernier
user-assigned-tickets-sort-oldest = Anciens
user-assigned-tickets-sort-oldest-desc = Les plus anciens d'abord, pour le tri
user-assigned-tickets-filter-high-priority = Priorité élevée uniquement
user-assigned-tickets-filter-new-activity = Nouvelle activité uniquement

# Tableau de bord : widget tickets récents (RecentTickets)
recent-tickets-empty = Aucun ticket récent
recent-tickets-context-open-new-tab = Ouvrir dans un nouvel onglet
recent-tickets-context-copy-link = Copier le lien
recent-tickets-context-remove = Retirer des récents

# Plugin admin: lifecycle state pills (PluginStateBadge)
plugin-state-active = Actif
plugin-state-disabled = Désactivé
plugin-state-quarantined = En quarantaine
plugin-state-uninstalled = Désinstallé

# Plugin admin: trust tier pills (PluginTrustBadge)
plugin-trust-official = Officiel
plugin-trust-verified = Vérifié
plugin-trust-community = Communauté
plugin-trust-local = Local

# Plugin admin: row card (PluginCard)
plugin-card-installed-on = Installé le { $date }
plugin-card-permissions = { $count ->
    [one] { $count } permission
   *[other] { $count } permissions
  }
plugin-card-sr-plugin-name = Nom du plugin
plugin-card-sr-installed = Installé
plugin-card-sr-permission-count = Nombre de permissions

# Plugin admin: detail view (PluginDetailView)
plugin-detail-back = Retour aux plugins
plugin-detail-loading = Chargement du plugin...
plugin-detail-loading-settings = Chargement des paramètres...
plugin-detail-lifecycle-heading = Cycle de vie
plugin-detail-settings-heading = Paramètres
plugin-detail-metadata-heading = Métadonnées
plugin-detail-metadata-source = Source
plugin-detail-metadata-permissions = Permissions
plugin-detail-metadata-permissions-count = { $count ->
    [one] { $count } déclarée
   *[other] { $count } déclarées
  }
plugin-detail-metadata-repository = Dépôt
plugin-detail-required-aria = requis
plugin-detail-secret-configured = Configuré
plugin-detail-secret-update = Modifier
plugin-detail-secret-cancel = Annuler
plugin-detail-secret-placeholder = Saisir une valeur
plugin-detail-secret-placeholder-new = Saisir une nouvelle valeur
plugin-detail-boolean-enabled = Activé
plugin-detail-action-enable = Activer
plugin-detail-action-disable = Désactiver
plugin-detail-action-uninstall = Désinstaller
plugin-detail-action-discard = Annuler
plugin-detail-action-save = Enregistrer
plugin-detail-action-saving = Enregistrement...
plugin-detail-status-missing-required = { $count ->
    [one] { $count } champ requis manquant
   *[other] { $count } champs requis manquants
  }
plugin-detail-status-unsaved = { $count ->
    [one] { $count } modification non enregistrée
   *[other] { $count } modifications non enregistrées
  }
plugin-detail-status-all-saved = Toutes les modifications sont enregistrées
plugin-detail-toast-saved = { $count ->
    [one] Paramètre enregistré
   *[other] Paramètres enregistrés
  }
plugin-detail-toast-enabled = Plugin activé
plugin-detail-toast-disabled = Plugin désactivé
plugin-detail-error-load = Échec du chargement du plugin
plugin-detail-error-save = Échec de l'enregistrement des paramètres. Réessayez.
plugin-detail-error-toggle = Échec du basculement du plugin
plugin-detail-error-uninstall = Échec de la désinstallation du plugin
plugin-detail-uninstall-title = Désinstaller le plugin
plugin-detail-uninstall-prompt-prefix = Désinstaller
plugin-detail-uninstall-prompt-mid = ? La politique
plugin-detail-uninstall-prompt-suffix = du plugin détermine si ses données sont conservées ou supprimées.
plugin-detail-uninstall-cancel = Annuler
plugin-detail-uninstall-confirm = Désinstaller

# Plugin admin: sideload view (PluginSideloadView)
plugin-sideload-back = Retour aux plugins
plugin-sideload-title = Charger une archive signée
plugin-sideload-intro-prefix = Pour les plugins qui ne sont pas encore dans le catalogue. L'archive doit être signée par un éditeur enregistré ou par la clé de signature locale de cette instance ; les envois non signés sont refusés. Vous cherchez un plugin officiel ? Parcourez d'abord le
plugin-sideload-intro-link = catalogue
plugin-sideload-intro-suffix = .
plugin-sideload-dropzone-aria = Choisir le fichier zip du plugin
plugin-sideload-choose-different = Choisir un autre fichier
plugin-sideload-drop-here = Déposez ici l'archive zip du plugin
plugin-sideload-or-browse = ou cliquez pour parcourir
plugin-sideload-warning-title = Ne chargez que des plugins provenant de sources de confiance.
plugin-sideload-warning-prefix = Une signature confirme que l'archive n'a pas été modifiée après signature, mais elle ne garantit pas les intentions de l'éditeur. Un plugin installé s'exécute dans l'interface d'administration avec accès à votre session. Préférez le
plugin-sideload-warning-link = catalogue
plugin-sideload-warning-suffix = pour les éditeurs vérifiés, et examinez la source de tout ce que vous chargez manuellement.
plugin-sideload-cancel = Annuler
plugin-sideload-install = Installer le plugin
plugin-sideload-installing = Installation...
plugin-sideload-error-not-zip = Veuillez sélectionner un fichier .zip
plugin-sideload-error-too-large = Le fichier doit faire moins de 2 Mo
plugin-sideload-error-install-failed = Échec de l'installation du plugin
# Vues invités et publiques (GuestTicketSubmitView, GuestTicketStatusView,
# PublicDocsView, PublicDocView, HelpView, PublicLayout,
# FeatureDisabledNotice). Textes affichés aux visiteurs non authentifiés.

# Partagé : notice de fonctionnalité désactivée et mise en page publique
feature-disabled-sign-in = Se connecter
public-layout-home-aria = Accueil de { $appName }
public-layout-logo-aria = Logo { $appName }
public-layout-nav-aria = Navigation publique
public-layout-docs-link = Documentation
public-layout-help-link = Aide

# Envoi de ticket invité
guest-submit-disabled-title = L'envoi de tickets n'est pas disponible
guest-submit-disabled-message = L'envoi de tickets par les invités est actuellement désactivé. Connectez-vous si vous avez un compte.
guest-submit-verify-title = Vérifiez votre boîte de réception
guest-submit-verify-message-prefix = Cliquez sur le lien de confirmation que nous avons envoyé à
guest-submit-verify-message-suffix = pour valider votre ticket et configurer votre portail.
guest-submit-verify-spam-hint = Rien reçu ? Vérifiez vos spams, puis réessayez dans quelques minutes.
guest-submit-another = Envoyer un autre ticket
guest-submit-success-title = Ticket bien reçu
guest-submit-success-email-prefix = Nous avons envoyé une confirmation à
guest-submit-success-email-suffix = avec un lien pour vous connecter et suivre l'avancement.
guest-submit-success-no-email = Votre ticket a bien été enregistré. Notre équipe vous recontactera par e-mail.
guest-submit-success-reference-prefix = Numéro de référence
guest-submit-track-heading = Suivre sans se connecter
guest-submit-copied = Copié
guest-submit-copy = Copier
guest-submit-track-hint = Conservez ce lien, c'est le seul moyen de consulter le ticket sans se connecter.
guest-submit-view-status = Voir l'état du ticket
guest-submit-another-short = Envoyer un autre
guest-submit-heading = Envoyer un ticket
guest-submit-tagline = Nous vous répondrons par e-mail.
guest-submit-honeypot-label = Site web
guest-submit-field-name = Votre nom
guest-submit-field-name-placeholder = Jeanne Dupont
guest-submit-field-email = Adresse e-mail
guest-submit-field-email-placeholder = vous@exemple.fr
guest-submit-field-title = Sujet
guest-submit-field-title-placeholder = Un bref résumé de votre demande
guest-submit-field-description = Description
guest-submit-field-description-placeholder = Expliquez-nous ce qui se passe et comment nous pouvons vous aider.
guest-submit-description-counter = { $count } / 10000
guest-submit-attachments-label = Pièces jointes
guest-submit-attachments-optional = (facultatif)
guest-submit-attachments-counter = { $count } / { $max }
guest-submit-attachments-uploading = Envoi en cours...
guest-submit-attachments-pick = Cliquez pour joindre un fichier
guest-submit-attachments-hint = Images, PDF ou texte. Jusqu'à { $size } Mo chacun.
guest-submit-attachments-remove-aria = Retirer { $name }
guest-submit-submitting = Envoi...
guest-submit-submit = Envoyer le ticket
guest-submit-have-account = Vous avez déjà un compte ?
guest-submit-sign-in = Se connecter
guest-submit-error-name = Veuillez saisir votre nom.
guest-submit-error-email = Veuillez saisir une adresse e-mail valide.
guest-submit-error-title = Veuillez saisir un sujet.
guest-submit-error-description = Veuillez décrire le problème.
guest-submit-error-uploads-pending = Veuillez attendre la fin des envois de fichiers.
guest-submit-error-rate-limited = Trop d'envois depuis votre réseau. Veuillez réessayer plus tard.
guest-submit-error-disabled = L'envoi de tickets a été désactivé.
guest-submit-error-account-exists = Un compte existe déjà pour cette adresse. Veuillez vous connecter pour envoyer un ticket.
guest-submit-error-generic = Échec de l'envoi du ticket. Veuillez réessayer.
guest-submit-error-network = Erreur réseau. Veuillez réessayer.
guest-submit-attach-error-max = { $max } pièces jointes au maximum.
guest-submit-attach-error-too-large = { $name } dépasse { $size } Mo.
guest-submit-attach-error-rate-limited = Trop d'envois depuis votre réseau. Réessayez plus tard.
guest-submit-attach-error-too-large-server = { $name } est trop volumineux.
guest-submit-attach-error-disabled = Les pièces jointes ne sont pas acceptées pour le moment.
guest-submit-attach-error-generic = Échec de l'envoi. Réessayez.
guest-submit-attach-error-network = Erreur réseau lors de l'envoi du fichier.
guest-submit-size-bytes = { $bytes } o
guest-submit-size-kb = { $value } Ko
guest-submit-size-mb = { $value } Mo

# État du ticket invité
guest-status-loading-aria = Chargement du ticket
guest-status-disabled-title = La consultation d'état n'est pas disponible
guest-status-disabled-message = La consultation de l'état des tickets invités est actuellement désactivée.
guest-status-ticket-number = Ticket n°{ $id }
guest-status-priority = Priorité
guest-status-opened = Ouvert le
guest-status-last-updated = Dernière mise à jour
guest-status-closed = Clôturé le
guest-status-reply-prefix = Vous souhaitez répondre ?
guest-status-reply-suffix = pour ajouter un commentaire.
guest-status-not-found-title = Ticket introuvable
guest-status-not-found-message = Le lien a peut-être expiré ou été mal saisi.

# Liste de documentation publique
public-docs-loading-aria = Chargement de la documentation
public-docs-disabled-title = La documentation n'est pas disponible
public-docs-disabled-message = La documentation publique est actuellement désactivée.
public-docs-heading = Documentation
public-docs-tagline = Parcourez les articles d'aide et tutoriels.
public-docs-search-placeholder = Rechercher dans la documentation...
public-docs-search-aria = Rechercher dans la documentation
public-docs-no-results = Aucun article ne correspond à votre recherche.
public-docs-empty = Aucune documentation disponible pour l'instant.
public-docs-updated = Mis à jour le { $date }

# Détail d'un article public
public-doc-loading-aria = Chargement de l'article
public-doc-back = Tous les documents
public-doc-last-updated = Dernière mise à jour le { $date }
public-doc-rich-text-prefix = Cet article utilise l'édition collaborative en texte riche. Une vue simplifiée est affichée ici, pour l'expérience complète avec commentaires et pièces jointes veuillez
public-doc-rich-text-link = vous connecter
public-doc-rich-text-suffix = .
public-doc-not-found-title = Document introuvable
public-doc-not-found-message = Il a peut-être été déplacé ou rendu privé.
public-doc-back-to-docs = Retour à la documentation

# Page d'aide
help-disabled-title = La page d'aide n'est pas disponible
help-disabled-message = La page d'aide en libre-service est actuellement désactivée.
help-heading = Comment pouvons-nous vous aider ?
help-tagline = Voici quelques actions possibles sans compte.
help-card-submit-title = Envoyer un ticket
help-card-submit-desc = Signalez un problème, nous vous répondrons par e-mail.
help-card-docs-title = Parcourir la documentation
help-card-docs-desc = Articles publics, guides et tutoriels.
help-card-reset-title = Réinitialiser votre mot de passe
help-card-reset-desc = Vous n'arrivez plus à accéder à votre compte ? C'est par ici.
help-card-signin-title = Se connecter
help-card-signin-desc = Vous avez déjà un compte ?

# Settings: appearance pane (AppearanceSettings).
settings-appearance-title = Apparence
settings-appearance-theme-heading = Thème
settings-appearance-theme-description = Choisissez votre palette de couleurs préférée
settings-appearance-device-local-label = Thème propre à l'appareil
settings-appearance-device-local-description = Ne pas synchroniser le thème entre les appareils (par exemple, utilisez le thème E-Paper sur votre tablette tout en gardant le mode sombre sur votre ordinateur portable)
settings-appearance-section-automatic = Automatique
settings-appearance-section-light = Thèmes clairs
settings-appearance-section-dark = Thèmes sombres
settings-appearance-red-horizon-easter-egg = Pourquoi leur infliger ça 😭
settings-appearance-accessibility-heading = Accessibilité
settings-appearance-accessibility-description = Améliorez la lisibilité et la distinction visuelle
settings-appearance-colorblind-label = Mode adapté au daltonisme
settings-appearance-colorblind-description-monochrome = Toujours activé pour les thèmes monochromatiques comme E-Paper et Red Horizon
settings-appearance-colorblind-description-default = Utiliser des formes distinctes pour les indicateurs de statut au lieu de se reposer uniquement sur les couleurs
settings-appearance-display-heading = Affichage
settings-appearance-display-description = Ajustez les préférences de mise en page
settings-appearance-compact-label = Vue compacte
settings-appearance-compact-description = Réduire l'espacement entre les éléments pour une mise en page plus dense
settings-appearance-theme-changed = Thème changé pour { $name }
settings-appearance-theme-changed-device-only = Thème changé pour { $name } (cet appareil uniquement)
settings-appearance-theme-save-failed = Échec de l'enregistrement du thème
settings-appearance-colorblind-toggled = Mode adapté au daltonisme { $state ->
    [enabled] activé
   *[disabled] désactivé
}
settings-appearance-device-local-toggled = Thème propre à l'appareil { $state ->
    [enabled] activé
   *[disabled] désactivé
}
settings-appearance-compact-toggled = Vue compacte { $state ->
    [enabled] activée
   *[disabled] désactivée
}
settings-appearance-system-theme-name = Système

# Settings: ThemeCard.
settings-appearance-card-system-name = Système

# Settings: security pane (SecuritySettings).
settings-security-title = Mot de passe
settings-security-label-current = Mot de passe actuel
settings-security-label-new = Nouveau mot de passe
settings-security-label-confirm = Confirmer le nouveau mot de passe
settings-security-placeholder-current = Saisissez votre mot de passe actuel
settings-security-placeholder-new = Saisissez votre nouveau mot de passe
settings-security-placeholder-confirm = Confirmez votre nouveau mot de passe
settings-security-placeholder-admin-new = Saisissez le nouveau mot de passe
settings-security-placeholder-admin-confirm = Confirmez le nouveau mot de passe
settings-security-hint-length = Le mot de passe doit comporter au moins 8 caractères
settings-security-error-mismatch = Les mots de passe ne correspondent pas
settings-security-submit-change = Changer le mot de passe
settings-security-submit-reset = Réinitialiser le mot de passe
settings-security-error-form-invalid = Veuillez remplir tous les champs correctement
settings-security-success-changed = Mot de passe modifié avec succès
settings-security-error-change-failed = Échec du changement de mot de passe. Vérifiez votre mot de passe actuel.
settings-security-success-reset = Mot de passe réinitialisé pour cet utilisateur
settings-security-error-reset-failed = Échec de la réinitialisation du mot de passe

# OAuth callback view + card.
auth-callback-loading-default = Finalisation de la connexion...
auth-callback-loading-processing = Traitement de l'authentification...
auth-callback-loading-success = Réussi ! Redirection...
auth-callback-loading-subtitle = Veuillez patienter pendant que nous terminons l'authentification
auth-callback-success-title = Authentification réussie
auth-callback-success-subtitle = Redirection...
auth-callback-technical-details = Détails techniques
auth-callback-provider-microsoft = Microsoft
auth-callback-provider-sso = SSO
auth-callback-error-missing-params = Paramètres d'authentification requis manquants
auth-callback-error-missing-detail = Manquant : { $fields }
auth-callback-error-missing-field-code = code
auth-callback-error-missing-field-state = état
auth-callback-error-invalid-response = Réponse du serveur invalide
auth-callback-error-no-response = Aucune réponse reçue du serveur
auth-callback-error-unknown = Erreur inconnue
auth-callback-error-generic-message = Une erreur inattendue s'est produite lors de l'authentification
auth-callback-error-status-prefix = Statut : { $status }
auth-callback-already-title = Compte déjà connecté
auth-callback-already-message = Ce compte { $provider } est déjà lié à un autre utilisateur dans le système.
auth-callback-already-suggestion-microsoft = Essayez de vous connecter avec un autre compte { $provider }, ou contactez votre administrateur.
auth-callback-already-suggestion-generic = Essayez de vous connecter avec un autre compte, ou contactez votre administrateur.
auth-callback-invalid-title = Échec de l'authentification
auth-callback-invalid-message = La demande d'authentification était invalide ou a expiré.
auth-callback-invalid-suggestion-microsoft = Veuillez réessayer de connecter votre compte { $provider }.
auth-callback-invalid-suggestion-generic = Veuillez réessayer de vous connecter.
auth-callback-generic-title = Échec de l'authentification
auth-callback-generic-suggestion = Veuillez réessayer ou contacter le support si le problème persiste.
auth-callback-action-try-different = Essayer un autre compte
auth-callback-action-back-settings = Retour aux paramètres
auth-callback-action-return-login = Retour à la connexion
auth-callback-action-try-again = Réessayer

# Widgets du tableau de bord
dashboard-widget-shell-action-view-all = Tout voir
dashboard-widget-shell-empty-title-default = Rien à afficher pour le moment.
dashboard-widget-shell-drag-label = Déplacer { $title }
dashboard-widget-shell-size-group-label = Taille de { $title }
dashboard-widget-shell-size-option-title = Taille { $size } sur 3
dashboard-widget-shell-hide-label = Masquer { $title }
dashboard-widget-shell-loading-label = Chargement de { $title }

dashboard-edit-bar-editing = Modification du tableau de bord
dashboard-edit-bar-add-widget = Ajouter un widget
dashboard-edit-bar-reset = Réinitialiser
dashboard-edit-bar-done = Terminé
dashboard-edit-bar-reset-confirm-title = Réinitialiser la disposition du tableau de bord ?
dashboard-edit-bar-reset-confirm-message = Votre disposition personnalisée sera remplacée par celle par défaut associée à votre rôle.
dashboard-edit-bar-reset-confirm-label = Réinitialiser

dashboard-add-widget-title = Ajouter un widget
dashboard-add-widget-all-added = Tous les widgets disponibles figurent déjà sur votre tableau de bord.

dashboard-staff-queue-title = File d'attente
dashboard-staff-queue-configure-aria = Configurer les indicateurs de la file
dashboard-staff-queue-configure-title = Configurer les indicateurs
dashboard-staff-queue-error = Impossible de charger les indicateurs de la file
dashboard-staff-queue-metric-unassigned-label = Non attribués
dashboard-staff-queue-metric-unassigned-desc = Ouverts, sans assigné
dashboard-staff-queue-metric-all-label = Tous les tickets
dashboard-staff-queue-metric-all-desc = Tous statuts confondus
dashboard-staff-queue-metric-open-label = Ouverts
dashboard-staff-queue-metric-open-desc = Statut : ouvert
dashboard-staff-queue-metric-in-progress-label = En cours
dashboard-staff-queue-metric-in-progress-desc = Actuellement traités
dashboard-staff-queue-metric-high-priority-label = Priorité haute
dashboard-staff-queue-metric-high-priority-desc = Priorité haute, encore ouverts
dashboard-staff-queue-metric-closed-today-label = Clôturés aujourd'hui
dashboard-staff-queue-metric-closed-today-desc = Clôturés dans les dernières 24 h

dashboard-staff-yours-title = Vos tickets
dashboard-staff-yours-error = Impossible de charger les compteurs
dashboard-staff-yours-assigned = Attribués
dashboard-staff-yours-open = Ouverts
dashboard-staff-yours-in-progress = En cours
dashboard-staff-yours-closed = Clôturés

dashboard-user-summary-title = Résumé
dashboard-user-summary-error = Impossible de charger le résumé
dashboard-user-summary-requests = Demandes
dashboard-user-summary-open = Ouvertes
dashboard-user-summary-in-progress = En cours
dashboard-user-summary-resolved = Résolues

dashboard-queue-metrics-picker-title = Configurer les indicateurs de la file
dashboard-queue-metrics-picker-hint = Choisissez jusqu'à { $max } indicateurs à afficher sur la carte File d'attente.
dashboard-queue-metrics-picker-count = ({ $count } / { $max } sélectionnés)
dashboard-queue-metrics-picker-toggle-aria = Activer { $label }
dashboard-queue-metrics-picker-cancel = Annuler
dashboard-queue-metrics-picker-save = Enregistrer

dashboard-knowledge-gaps-title = Lacunes de connaissances
dashboard-knowledge-gaps-title-with-count = Lacunes de connaissances ({ $count })
dashboard-knowledge-gaps-action = Voir la file
dashboard-knowledge-gaps-error = Impossible de charger les lacunes
dashboard-knowledge-gaps-empty-title = Aucune lacune ouverte
dashboard-knowledge-gaps-empty-description = Les tickets signalés pour la documentation apparaîtront ici.
dashboard-knowledge-gaps-signal-count = { $count -> [one] 1 signal *[other] { $count } signaux }
dashboard-knowledge-gaps-impact-tickets = { $count } tickets
dashboard-knowledge-gaps-impact-searches = { $count } recherches
dashboard-knowledge-gaps-impact-tooltip-tickets = { $count } tickets révélant un besoin pour ce document
dashboard-knowledge-gaps-impact-tooltip-searches = { $count } recherches révélant un besoin pour ce document

dashboard-channel-health-title = État des canaux
dashboard-channel-health-action = Gérer
dashboard-channel-health-error = Impossible de charger les canaux
dashboard-channel-health-empty-title = Aucun canal configuré
dashboard-channel-health-empty-description = Ajoutez un canal e-mail pour collecter des tickets.
dashboard-channel-health-status-disabled = Désactivé
dashboard-channel-health-status-error = Erreur
dashboard-channel-health-status-healthy = Opérationnel
dashboard-channel-health-polled = interrogé { $time }
dashboard-channel-health-never-polled = jamais interrogé

dashboard-my-devices-title = Mes appareils
dashboard-my-devices-error = Impossible de charger les appareils
dashboard-my-devices-empty-title = Aucun appareil attribué
dashboard-my-devices-empty-description = Les appareils liés à votre compte apparaîtront ici.
dashboard-my-devices-unknown-model = Modèle inconnu

dashboard-recently-viewed-title = Récemment consultés
dashboard-recently-viewed-error = Impossible de charger les éléments récents
dashboard-recently-viewed-empty-title = Rien pour l'instant
dashboard-recently-viewed-empty-description = Les tickets que vous ouvrirez apparaîtront ici.

dashboard-starred-docs-title = Documents favoris
dashboard-starred-docs-error = Impossible de charger les favoris
dashboard-starred-docs-empty-title = Aucune page favorite
dashboard-starred-docs-empty-description = Marquez un document d'une étoile pour le retrouver vite.

dashboard-unassigned-queue-title = File non attribuée
dashboard-unassigned-queue-error = Impossible de charger la file
dashboard-unassigned-queue-empty-title = Boîte vide
dashboard-unassigned-queue-empty-description = Rien n'attend dans la file.

# Couche UI de base
ui-site-header-untitled-ticket = Ticket sans titre
ui-site-header-unknown-device = Appareil inconnu
ui-site-header-ticket-title-placeholder = Saisir le titre du ticket...
ui-site-header-document-title-placeholder = Saisir le titre du document...
ui-site-header-create-aria = Créer { $action }
ui-site-header-inbox-tooltip = Boîte de réception
ui-site-header-inbox-aria = Ouvrir la boîte de réception
ui-user-selection-modal-title = Affecter un utilisateur
ui-user-selection-modal-search-placeholder = Rechercher par nom ou e-mail...
ui-user-selection-modal-unassign = Désaffecter l'utilisateur
ui-user-selection-modal-error = Échec du chargement des utilisateurs
ui-user-selection-modal-empty-no-match = Aucun utilisateur trouvé
ui-user-selection-modal-empty-no-users = Aucun utilisateur disponible
ui-user-selection-modal-role-admin = Administrateur
ui-user-selection-modal-role-technician = Technicien
ui-user-selection-modal-role-user = Utilisateur
ui-user-card-role-admin = Administrateur
ui-user-card-role-technician = Technicien
ui-user-card-role-user = Utilisateur
ui-quick-tooltip-unassigned = Non affecté
ui-quick-tooltip-status-label = Statut :
ui-quick-tooltip-requester-label = Demandeur :
ui-quick-tooltip-assignee-label = Affecté à :
ui-presence-stack-fallback-name = Quelqu'un
ui-presence-stack-aria = { $count -> [one] { $count } personne en consultation *[other] { $count } personnes en consultation }
ui-presence-stack-overflow-title = { $count -> [one] { $count } autre en consultation *[other] { $count } autres en consultation }
ui-status-badge-status-open = ouvert
ui-status-badge-status-in-progress = en cours
ui-status-badge-status-closed = fermé
ui-status-badge-priority-low = basse
ui-status-badge-priority-medium = moyenne
ui-status-badge-priority-high = haute
ui-status-badge-priority-low-full = priorité basse
ui-status-badge-priority-medium-full = priorité moyenne
ui-status-badge-priority-high-full = priorité haute
ui-heatmap-tooltip-more = ...et { $count -> [one] { $count } de plus *[other] { $count } de plus }
ui-device-groups-title = Groupes
ui-header-title-placeholder = Saisir un titre...

# Search + ticket remnants
search-global-filter-documentation = Documentation
search-global-filter-tickets = Tickets
search-global-filter-devices = Appareils
search-global-filter-users = Utilisateurs
search-global-placeholder = Rechercher tickets, docs, appareils, utilisateurs
search-global-placeholder-filtered = Rechercher dans { $filter }
search-global-aria-label = Recherche
search-global-prompt-title = Recherchez dans votre helpdesk
search-global-prompt-subtitle = Trouvez des tickets, de la documentation, des appareils et plus encore
search-global-empty-prefix = Aucun résultat pour
search-global-empty-hint = Essayez d'autres mots-clés ou vérifiez l'orthographe
search-global-hint-navigate = Naviguer
search-global-hint-open = Ouvrir
search-global-hint-close = Fermer
search-global-results-count = { $count -> [one] { $count } résultat *[other] { $count } résultats }
search-global-results-took = { $ms } ms
search-result-item-today = Aujourd'hui
search-result-item-yesterday = Hier
search-result-item-days-ago = il y a { $count } j
search-result-item-weeks-ago = il y a { $count } sem
search-result-item-months-ago = il y a { $count } mois
search-result-item-years-ago = il y a { $count } an
search-result-item-internal-title = Note interne, visible uniquement par le personnel
search-result-item-internal-badge = Interne
search-result-group-ticket = Tickets
search-result-group-comment = Commentaires
search-result-group-documentation = Documentation
search-result-group-attachment = Pièces jointes
search-result-group-device = Appareils
search-result-group-user = Utilisateurs
tickets-cycle-burndown-load-error = Échec du chargement des statistiques
tickets-cycle-burndown-cat-triage = Tri
tickets-cycle-burndown-cat-backlog = Backlog
tickets-cycle-burndown-cat-active = En cours
tickets-cycle-burndown-cat-in-review = En relecture
tickets-cycle-burndown-cat-done = Terminé
tickets-cycle-burndown-cat-cancelled = Annulé
tickets-cycle-burndown-frozen = Figé
tickets-cycle-burndown-live = En direct
tickets-cycle-burndown-loading = Chargement...
tickets-cycle-burndown-tickets-done = Tickets terminés
tickets-cycle-burndown-complete = Terminé
tickets-cycle-burndown-days-remaining = { $count -> [one] Jour restant *[other] Jours restants }
tickets-cycle-burndown-snapshot-frozen = Instantané figé { $date }
tickets-collaborative-article-title = Notes du ticket
tickets-collaborative-article-doc-title = Documentation : Ticket n°{ $id }
tickets-collaborative-article-revision-history = Historique des révisions
tickets-collaborative-article-convert-doc = Convertir en page de documentation
tickets-collaborative-article-open-full = Ouvrir l'éditeur complet
tickets-project-info-remove = Retirer du projet
tickets-project-info-description = Description
tickets-project-info-project-id = ID projet
tickets-project-info-status = Statut
tickets-project-info-tickets = Tickets
tickets-project-info-print-tickets = { $count -> [one] { $count } ticket *[other] { $count } tickets }
tickets-project-info-status-active = actif
tickets-project-info-status-completed = terminé
tickets-project-info-status-archived = archivé
tickets-email-html-iframe-title = Corps de l'e-mail
tickets-email-html-show-less = Réduire
tickets-email-html-show-full = Afficher l'e-mail complet
tickets-email-html-scaled = Mis à l'échelle ({ $pct } %)

# Header + nav polish
nav-logo-alt = Logo Nosdesk
nav-logo-alt-collapsed = Nosdesk
nav-section-recent-tickets = Tickets récents
nav-section-documentation = Documentation
nav-more-heading = Plus de navigation
common-search-placeholder = Rechercher...
header-create-ticket = Créer un ticket
header-create-project = Créer un projet
header-add-ticket = Ajouter un ticket
header-create-user = Créer un utilisateur
header-create-device = Créer un appareil
header-create-document = Créer un document
nav-route-announcement = Vous êtes sur { $title }
common-dropdown-select-placeholder = Sélectionner une option
common-dropdown-empty-message = Aucun résultat

# Inbox + notifications polish
inbox-group-today = Aujourd'hui
inbox-group-yesterday = Hier
inbox-group-this-week = Cette semaine
inbox-group-earlier = Plus ancien
inbox-mark-mentions-read = Marquer les mentions comme lues
inbox-mark-all-read = Tout marquer comme lu
inbox-empty-caught-up-title = Vous êtes à jour
inbox-empty-caught-up-subtitle = Rien à lire pour le moment. Les nouvelles notifications apparaîtront ici dès leur arrivée.
inbox-empty-mentions-title = Aucune mention pour l'instant
inbox-empty-mentions-subtitle = Lorsque quelqu'un vous @mentionne dans un commentaire, vous le verrez ici.
inbox-empty-default-title = Aucune notification pour l'instant
inbox-empty-default-subtitle = Les mises à jour des tickets, commentaires, mentions et documents que vous suivez s'afficheront ici.
inbox-footer-loading-more = Chargement...
inbox-footer-end-of-feed = Fin du fil
notifications-bell-header = Notifications
notifications-bell-open-inbox = Ouvrir la boîte de réception
notifications-bell-loading = Chargement...
notifications-bell-load-more = Charger plus
notifications-bell-mark-mentions-read = Marquer les mentions comme lues
notifications-bell-mark-all-read = Tout marquer comme lu
notifications-bell-settings = Paramètres
notifications-filter-tabs-all = Toutes
notifications-filter-tabs-unread = Non lues
notifications-filter-tabs-mentions = Mentions
notifications-toast-new-with-title = Nouvelle notification : { $title } ({ $seq })
notifications-toast-new = Nouvelle notification ({ $seq })
# Route meta titles
route-title-login = Connexion
route-title-reset-password = Réinitialiser le mot de passe
route-title-mfa-setup = Configuration de la MFA requise
route-title-accept-invitation = Accepter l'invitation
route-title-onboarding = Configuration
route-title-guest-submit-ticket = Soumettre un ticket
route-title-guest-ticket-status = Statut du ticket
route-title-documentation = Documentation
route-title-help = Aide
route-title-dashboard = Tableau de bord
route-title-inbox = Boîte de réception
route-title-tickets = Tickets
route-title-ticket-view = Voir le ticket
route-title-ticket-notes = Notes du ticket #{ $id }
route-title-user-profile = Profil utilisateur
route-title-user-settings = Paramètres utilisateur
route-title-user-settings-profile = Paramètres du profil utilisateur
route-title-user-settings-appearance = Paramètres d'apparence de l'utilisateur
route-title-user-settings-notifications = Paramètres de notification de l'utilisateur
route-title-user-settings-security = Paramètres de sécurité de l'utilisateur
route-title-projects = Projets
route-title-cycles = Cycles
route-title-cycle-detail = Cycle
route-title-project-gantt = Gantt
route-title-assets = Ressources
route-title-project-detail = Détails du projet
route-title-error = Erreur
route-title-users = Utilisateurs
route-title-devices = Appareils
route-title-device-create = Créer un appareil
route-title-device-view = Détails de l'appareil
route-title-documentation-drafts = Brouillons
route-title-collection = Collection
route-title-documentation-archived = Archivés
route-title-documentation-trash = Corbeille
route-title-knowledge-gaps = Lacunes de connaissances
route-title-profile = Profil
route-title-profile-settings = Paramètres
route-title-profile-settings-profile = Paramètres du profil
route-title-profile-settings-appearance = Paramètres d'apparence
route-title-profile-settings-notifications = Paramètres de notification
route-title-profile-settings-security = Paramètres de sécurité
route-title-administration = Administration
route-title-admin-groups = Groupes
route-title-group-configuration = Configuration du groupe
route-title-admin-categories = Catégories
route-title-admin-assignment-rules = Règles d'attribution
route-title-admin-workflow = Flux de travail
route-title-admin-api-tokens = Jetons d'API
route-title-admin-webhooks = Webhooks
route-title-admin-sla = SLA
route-title-admin-plugins = Extensions
route-title-admin-plugin-registry = Registre des extensions
route-title-admin-plugin-sideload = Installer une extension
route-title-admin-plugin-detail = Détails de l'extension
route-title-admin-auth-providers = Fournisseurs d'authentification
route-title-admin-search = Gestion de l'index de recherche
route-title-admin-system-settings = Paramètres système
route-title-admin-branding = Image de marque
route-title-admin-audit-log = Journal d'audit
route-title-admin-email-queue = File d'attente des e-mails
route-title-admin-email-suppressions = Suppressions d'e-mails
route-title-admin-guest-access = Accès invité
route-title-admin-email-settings = Configuration des e-mails
route-title-admin-channels-email = Réception des e-mails
route-title-admin-data-import = Importation de données
route-title-admin-microsoft-graph = Connexion Microsoft Graph
route-title-admin-csv-import = Importation CSV
route-title-admin-backup-restore = Sauvegarde et restauration
route-title-group-detail = Détails du groupe
route-title-authenticating = Authentification en cours...
route-title-pdf-viewer = Visionneuse PDF

# Loading modals + scattered polish
common-loading-projects = Chargement des projets...
common-loading-devices = Chargement des appareils...
common-loading-generic = Chargement...
common-loading-groups = Chargement des groupes...
common-delete-item-aria = Supprimer { $name }
admin-branding-aria-logo = Logo
admin-branding-aria-logo-light = Logo pour thème clair
admin-branding-aria-favicon = Favicon

# Store error messages
error-store-workflow-states-load = Impossible de charger les états du flux de travail.
error-store-public-settings-load = Impossible de charger les paramètres publics.
error-store-feature-flags-load = Impossible de charger les indicateurs de fonctionnalité.
error-store-recent-tickets-load = Impossible de récupérer les tickets récents.
error-store-saved-views-load = Impossible de charger les vues enregistrées.
error-store-saved-view-save = Impossible d'enregistrer la vue.
error-store-saved-view-update = Impossible de mettre à jour la vue.
error-store-saved-view-delete = Impossible de supprimer la vue.
error-store-cycles-load = Impossible de charger les cycles.
error-store-cycle-create = Impossible de créer le cycle.
error-store-cycle-update = Impossible de mettre à jour le cycle.
error-store-cycle-complete = Impossible de terminer le cycle.
error-store-cycle-archive = Impossible d'archiver le cycle.
error-store-auth-profile-load = Impossible de charger votre profil. Veuillez réessayer.
error-store-auth-mfa-setup-start = Impossible de démarrer la configuration MFA. Veuillez réessayer.
error-store-auth-mfa-setup-complete = Impossible de terminer la configuration MFA. Veuillez réessayer.

# Q2 polish: helpers + registries + errors
priority-urgent = Urgent
priority-high = Haute
priority-medium = Moyenne
priority-low = Basse
priority-none = Aucune priorité
sla-breached = Dépassé
sla-at-risk = À risque
sla-on-track = Dans les temps
sla-paused = En pause
sla-none = Pas de SLA
status-open = Ouvert
status-in-progress = En cours
status-closed = Fermé
status-unknown = Inconnu
color-red = Rouge
color-orange = Orange
color-yellow = Jaune
color-green = Vert
color-cyan = Cyan
color-blue = Bleu
color-purple = Violet
color-pink = Rose
priority-indicator-low-aria = Priorité basse
priority-indicator-medium-aria = Priorité moyenne
priority-indicator-high-aria = Priorité haute
priority-indicator-unknown-aria = Priorité inconnue
search-entity-type-tickets = Tickets
search-entity-type-comments = Commentaires
search-entity-type-documentation = Documentation
search-entity-type-attachments = Pièces jointes
search-entity-type-devices = Appareils
search-entity-type-users = Utilisateurs
plugin-permission-ticket-read-label = Lire les tickets
plugin-permission-ticket-read-description = Lire les données des tickets
plugin-permission-ticket-write-label = Écrire les tickets
plugin-permission-ticket-write-description = Créer et mettre à jour des tickets
plugin-permission-ticket-comment-label = Commenter les tickets
plugin-permission-ticket-comment-description = Ajouter des commentaires aux tickets
plugin-permission-ticket-delete-label = Supprimer les tickets
plugin-permission-ticket-delete-description = Supprimer des tickets
plugin-permission-device-read-label = Lire les appareils
plugin-permission-device-read-description = Lire les données des appareils
plugin-permission-device-write-label = Écrire les appareils
plugin-permission-device-write-description = Créer et mettre à jour des appareils
plugin-permission-user-read-label = Lire les utilisateurs
plugin-permission-user-read-description = Lire les données de profil utilisateur
plugin-permission-storage-plugin-label = Stockage du plugin
plugin-permission-storage-plugin-description = Stocker des données clé-valeur propres au plugin
plugin-permission-collection-read-label = Lire les collections
plugin-permission-collection-read-description = Lire les enregistrements de collections typées
plugin-permission-collection-write-label = Écrire les collections
plugin-permission-collection-write-description = Créer et mettre à jour les enregistrements de collections typées
error-resource-not-found = La ressource demandée est introuvable.
error-network = Impossible de se connecter au serveur. Veuillez vérifier votre connexion Internet.
error-session-expired = Votre session a expiré. Veuillez vous reconnecter.
error-forbidden = Vous n'avez pas l'autorisation d'effectuer cette action.
plugin-error-load-failed = Échec du chargement du plugin
plugin-error-pending-review = Ce plugin est en attente de validation
plugin-error-not-installed = Composant du plugin non installé
plugin-error-component-not-found = Composant introuvable dans le plugin
plugin-error-timeout = Le plugin a mis trop de temps à charger
plugin-error-failed = Échec du chargement du plugin
bulk-action-undo = Annuler
bulk-action-undone = Annulé
bulk-action-undo-failed = Échec de l'annulation
passkey-last-used-never = Jamais

# Dashboard widget registry
dashboard-widget-assigned-tickets-title = Tickets assignés
dashboard-widget-assigned-tickets-description = Votre file de travail actuelle avec statut et priorité.
dashboard-widget-stats-yours-title = Vos compteurs
dashboard-widget-stats-yours-description = Compteurs rapides des tickets qui vous sont assignés, par statut.
dashboard-widget-stats-queue-title = Compteurs de file
dashboard-widget-stats-queue-description = Tickets non assignés et totaux dans la file.
dashboard-widget-unassigned-queue-title = File non assignée
dashboard-widget-unassigned-queue-description = Tickets ouverts les plus anciens sans assigné. Prenez le suivant.
dashboard-widget-recently-viewed-title = Récemment consultés
dashboard-widget-recently-viewed-description = Tickets que vous avez consultés récemment.
dashboard-widget-starred-docs-title = Documents favoris
dashboard-widget-starred-docs-description = Pages de documentation que vous avez mises en favori.
dashboard-widget-my-devices-title = Mes appareils
dashboard-widget-my-devices-description = Appareils dont vous êtes l'utilisateur principal.
dashboard-widget-channel-health-title = Santé des canaux
dashboard-widget-channel-health-description = État des canaux email entrants : dernier sondage, activation, erreurs.
dashboard-widget-activity-heatmap-title = Carte d'activité
dashboard-widget-activity-heatmap-description = Carte 365 jours des tickets que vous avez fermés.
dashboard-widget-activity-heatmap-prop-title = Votre activité
dashboard-widget-requested-tickets-title = Vos demandes
dashboard-widget-requested-tickets-description = Tickets que vous avez ouverts avec leur statut.
dashboard-widget-requested-tickets-prop-title = Vos demandes
dashboard-widget-stats-summary-title = Résumé des demandes
dashboard-widget-stats-summary-description = Nombre de vos demandes par statut.
dashboard-widget-knowledge-gaps-title = Lacunes documentaires
dashboard-widget-knowledge-gaps-description = Principaux articles à rédiger, classés selon les tickets.
# Q1 polish: template attributes + static text
tickets-row-new-activity-tooltip = Nouvelle activité depuis votre dernière consultation
tickets-row-new-activity-aria = Nouvelle activité
common-confirm-delete-title = Confirmer la suppression
common-toast-dismiss = Fermer
common-error-banner-dismiss = Fermer l'erreur
common-route-progress-aria = Chargement
common-bulk-actions-aria = Actions groupées
common-loading-more-aria = Chargement de la suite
pagination-controls-page = Page
pagination-controls-show = Afficher
pagination-controls-id-placeholder = ID
device-modal-title = Sélectionner un appareil
device-modal-search-placeholder = Rechercher par nom, hôte, numéro de série, fabricant ou utilisateur...
device-modal-owner = Propriétaire
device-modal-unassigned = Non attribué
device-modal-col-device = Appareil
device-modal-col-status = Statut
device-modal-col-serial = Série
device-modal-col-user = Utilisateur
project-modal-title = Ajouter au projet
project-modal-search-placeholder = Rechercher des projets par nom ou description...
project-modal-col-name = Nom du projet
project-modal-col-description = Description
project-modal-col-status = Statut
project-modal-col-tickets = Tickets
project-modal-col-action = Action
kanban-recurring-tooltip = Ticket récurrent
kanban-recurring-aria = Récurrent
kanban-sla-aria = Statut SLA
calendar-today = Aujourd'hui
calendar-anchor-label = Ancre
calendar-anchor-tooltip = Le champ d'ancrage est défini par la vue enregistrée; le sélecteur arrivera dans une prochaine version.
calendar-anchor-due-date = Échéance
calendar-anchor-created = Création
calendar-anchor-last-activity = Dernière activité
gantt-today = Aujourd'hui
gantt-title = Gantt
user-cell-missing-tooltip = Cet utilisateur n'existe plus
user-cell-unknown = Inconnu
user-settings-managing-for = Gestion des paramètres pour
user-settings-groups-title = Groupes
user-settings-role-management-title = Gestion des rôles
user-settings-account-setup-title = Configuration du compte
user-settings-account-setup-pending = En attente
user-settings-invitation-pending = Invitation en attente
user-settings-resend-invitation-title = Renvoyer l'invitation par e-mail
user-settings-danger-zone-title = Zone dangereuse
user-settings-danger-zone-subtitle = Actions irréversibles et destructrices
user-settings-delete-modal-title = Confirmer la suppression du compte
user-settings-delete-item-profile = Informations et paramètres du profil
user-settings-delete-item-tickets = Tous les tickets créés ou attribués à cet utilisateur
user-settings-delete-item-comments = Commentaires et historique d'activité
user-settings-delete-item-access = Accès à tous les systèmes et ressources
user-settings-password-placeholder = Saisissez votre mot de passe
admin-plugins-list-title = Plugins
admin-plugins-list-aria-filter = Filtrer les plugins
admin-plugins-list-search-placeholder = Rechercher des plugins
admin-plugins-list-uninstall-title = Désinstaller le plugin
inbox-title = Boîte de réception
inbox-aria-filter = Filtrer les notifications
inbox-aria-bulk-actions = Actions groupées
inbox-aria-clear-selection = Effacer la sélection
inbox-aria-select-all = Tout sélectionner
notifications-bell-aria-trigger = Notifications
notifications-bell-aria-filter = Filtrer les notifications
editor-mentions-hint-select = Sélectionner
editor-mentions-hint-close = Fermer
editor-mentions-helper-type = Tapez
editor-mentions-helper-suffix = pour mentionner quelqu'un

# R1 auth UX errors
auth-mfa-check-failed = Échec de la vérification du statut MFA
auth-mfa-setup-failed = Échec de la configuration de la MFA
auth-mfa-setup-failed-retry = La configuration de la MFA a échoué. Veuillez réessayer.
auth-mfa-code-invalid = Veuillez saisir un code valide à 6 chiffres
auth-mfa-secret-missing = Le secret MFA est manquant. Veuillez relancer la configuration.
auth-mfa-verify-failed = Code de vérification invalide. Veuillez réessayer.
auth-mfa-enable-failed = Échec de l'activation de la MFA
auth-mfa-disable-failed = Échec de la désactivation de la MFA
auth-mfa-backup-codes-failed = Échec de la régénération des codes de secours
auth-passkey-load-failed = Échec du chargement des clés d'accès
auth-passkey-not-supported-browser = Les clés d'accès ne sont pas prises en charge par ce navigateur
auth-passkey-not-supported-device = Les clés d'accès ne sont pas prises en charge par cet appareil
auth-passkey-max-reached = Nombre maximal de clés d'accès atteint (10)
auth-passkey-registered = Clé d'accès « { $name } » enregistrée avec succès
auth-passkey-registration-not-allowed = L'enregistrement a été annulé ou refusé
auth-passkey-already-registered = Cette clé d'accès est déjà enregistrée
auth-passkey-registration-cancelled = Enregistrement annulé
auth-passkey-register-failed = Échec de l'enregistrement de la clé d'accès
auth-passkey-login-success = Connexion réussie avec la clé d'accès
auth-passkey-auth-not-allowed = L'authentification a été annulée ou refusée
auth-passkey-none-registered = Aucune clé d'accès enregistrée pour ce compte
auth-passkey-auth-cancelled = Authentification annulée
auth-passkey-login-failed = Échec de la connexion avec la clé d'accès
auth-passkey-name-required = Le nom de la clé d'accès est requis
auth-passkey-name-too-long = Le nom de la clé d'accès doit faire 100 caractères ou moins
auth-passkey-renamed = Clé d'accès renommée
auth-passkey-rename-failed = Échec du renommage de la clé d'accès
auth-passkey-delete-password-required = Mot de passe requis pour supprimer une clé d'accès
auth-passkey-deleted = Clé d'accès supprimée
auth-passkey-incorrect-password = Mot de passe incorrect
auth-passkey-delete-failed = Échec de la suppression de la clé d'accès
ticket-data-load-failed = Impossible de charger le ticket. Veuillez réessayer plus tard.
plugins-load-failed = Échec du chargement des plugins
search-failed = La recherche a échoué. Veuillez réessayer.
auth-autologin-prompt = Veuillez vous connecter avec vos identifiants.
auth-login-rate-limited = Trop de requêtes. Veuillez patienter un instant.
auth-login-network-error = Erreur réseau. Veuillez vérifier votre connexion.
auth-login-backup-codes-low = Connexion réussie. Pensez à régénérer vos codes de secours, il vous en reste 2 ou moins.
ticket-audio-play-failed = Échec de la lecture audio
device-modal-load-failed = Échec du chargement des appareils. Veuillez réessayer.
project-modal-load-failed = Échec du chargement des projets. Veuillez réessayer plus tard.
user-profile-load-failed = Échec du chargement des informations de l'utilisateur
# R2 filter facets + ticket columns
filter-facet-title = Titre
filter-facet-status = Statut
filter-facet-priority = Priorité
filter-facet-assignee = Assigné
filter-facet-sla = SLA
filter-facet-cycle = Cycle
filter-assignee-unassigned = Non assigné
filter-assignee-loading = Chargement…
filter-cycle-option = Cycle nº { $id }
filter-summary-n-selected = { $count } sélectionnés
tickets-column-id = N°
tickets-column-id-description = Numéro du ticket
tickets-column-title = Titre
tickets-column-title-description = Sujet du ticket
tickets-column-status = Statut
tickets-column-status-description = État du workflow
tickets-column-priority = Priorité
tickets-column-priority-description = Priorité
tickets-column-assignee = Assigné à
tickets-column-assignee-description = Propriétaire du ticket
tickets-column-requester = Demandeur
tickets-column-requester-description = Personne ayant signalé le ticket
tickets-column-category = Catégorie
tickets-column-category-description = Étiquette de catégorie du ticket
tickets-column-cycle = Cycle
tickets-column-cycle-description = Appartenance au cycle
tickets-column-due-date = Échéance
tickets-column-due-date-description = Date limite
tickets-column-last-activity = Mis à jour
tickets-column-last-activity-description = Dernière modification du ticket
tickets-column-created-at = Créé
tickets-column-created-at-description = Date d'ouverture du ticket
tickets-column-sla = SLA
tickets-column-sla-description = Pastille SLA (vert / ambre / rouge)
tickets-column-kb-gap = KB
tickets-column-kb-gap-description = Signal de lacune dans la base de connaissances
tickets-column-devices = Appareils
tickets-column-devices-description = Nombre d'appareils concernés
tickets-column-recurrence = Récurr.
tickets-column-recurrence-description = Marqueur de ticket récurrent

# Backend HTTP error responses (R3)
backend-error-auth-required = Authentification requise.
backend-error-user-not-found = Compte utilisateur introuvable.
backend-error-comment-fetch-failed = Impossible de charger les commentaires.
backend-error-comment-create-failed = Impossible de créer le commentaire.
backend-error-comment-not-found = Commentaire introuvable.
backend-error-attachment-not-found = Pièce jointe introuvable.
backend-error-attachment-delete-failed = Impossible de supprimer la pièce jointe.

# S2 backend handler error sweep
backend-error-validation = Erreur de validation.
backend-error-passkey-max-reached = Nombre maximal de clés d'accès atteint.
backend-error-bad-request = Requête incorrecte.
backend-error-search-failed = Échec de la recherche.
backend-error-search-rebuild-failed = Échec de la reconstruction de l'index.

# S1 frontend registries + defaults
builtin-view-my-open-name = Mes tickets ouverts
builtin-view-my-open-description = Tickets ouverts qui vous sont attribués
builtin-view-all-active-name = Tous actifs
builtin-view-all-active-description = Tous les tickets non résolus ni annulés
builtin-view-triage-name = Tri initial
builtin-view-triage-description = Tickets en attente de première catégorisation
builtin-view-calendar-name = Calendrier
builtin-view-calendar-description = Tickets placés à leur date d'échéance
workflow-category-triage = Tri
workflow-category-backlog = File d'attente
workflow-category-active = Actif
workflow-category-in-review = En relecture
workflow-category-done = Terminé
workflow-category-cancelled = Annulé
assignment-method-direct-user-name = Utilisateur direct
assignment-method-direct-user-description = Attribuer directement à un utilisateur précis
assignment-method-group-round-robin-name = Rotation (groupe)
assignment-method-group-round-robin-description = Répartir équitablement entre les membres du groupe
assignment-method-group-random-name = Aléatoire (groupe)
assignment-method-group-random-description = Choisir un membre du groupe au hasard pour chaque ticket
assignment-method-group-queue-name = File de groupe
assignment-method-group-queue-description = Placer dans la file du groupe (les utilisateurs prennent les tickets)
tickets-category-none = Aucune catégorie
tickets-menu-flag-for-docs = Signaler pour documentation
tickets-menu-delete = Supprimer le ticket
docs-author-system = Système
docs-untitled-page = Sans titre
profile-role-user-label = Utilisateur
profile-role-user-description = Peut créer des tickets et consulter les ressources qui lui sont attribuées
profile-role-technician-label = Technicien
profile-role-technician-description = Peut gérer les tickets, les appareils et aider les autres utilisateurs
profile-role-admin-label = Administrateur
profile-role-admin-description = Accès complet à toutes les fonctionnalités et à la gestion des utilisateurs
tickets-grouping-no-cycle = Aucun cycle
tickets-grouping-all = Tous

# T batch: final sweep
error-api-server = Une erreur serveur s'est produite. Veuillez réessayer plus tard.
error-api-validation = Les données fournies ne sont pas valides.
error-api-generic = Une erreur s'est produite lors du traitement de votre demande.
plugin-loader-error = Impossible de charger les plugins.
seed-welcome-page-title = Bienvenue dans Nosdesk
email-notice-security = Avis de sécurité
email-notice-security-critical = Avis de sécurité critique
email-notice-getting-started = Pour commencer
email-notice-success = Succès
email-link-fallback-prompt = Ou copiez et collez ce lien dans votre navigateur :
email-footer-rights = Tous droits réservés.
email-footer-automated = Ceci est un message automatique. Veuillez ne pas y répondre directement.
markdown-embed-depth-limit = [Limite de profondeur d'intégration atteinte]
markdown-embed-circular = [Intégration circulaire détectée]
markdown-embed-reference = Intégré : { $title }
markdown-embed-reference-fallback = [Intégré : { $title }]

# V batch: editor plugins + SSE
editor-embed-empty-document = Document vide
editor-embed-load-failed = Impossible de charger le document
editor-embed-open-document = Ouvrir le document
editor-loading = Chargement…
sse-connection-failed = Échec de la connexion.
sse-no-auth-token = Non authentifié.
auth-microsoft-logout-failed = Impossible de se déconnecter de Microsoft.
editor-ticket-link-not-found = Ticket #{ $id } introuvable

# W batch: pluralization fixes
notifications-inbox-unread-count = { $count -> [one] { $count } notification non lue *[other] { $count } notifications non lues }
gantt-tickets-in-view = { $count -> [one] { $count } ticket affiché *[other] { $count } tickets affichés }
bulk-bar-select-all-matching = Tout sélectionner ({ $count })
bulk-bar-clear = Effacer
bulk-bar-selected-generic = { $count -> [one] { $count } sélectionné *[other] { $count } sélectionnés }
bulk-bar-all-selected-generic = Les { $count } sont sélectionnés
bulk-bar-tickets-selected = { $count -> [one] { $count } ticket sélectionné *[other] { $count } tickets sélectionnés }
bulk-bar-tickets-all-selected = Les { $count } tickets sont sélectionnés
bulk-bar-users-selected = { $count -> [one] { $count } utilisateur sélectionné *[other] { $count } utilisateurs sélectionnés }
bulk-bar-users-all-selected = Les { $count } utilisateurs sont sélectionnés
bulk-bar-devices-selected = { $count -> [one] { $count } appareil sélectionné *[other] { $count } appareils sélectionnés }
bulk-bar-devices-all-selected = Les { $count } appareils sont sélectionnés
inbox-no-unread = Vous n'avez aucune notification non lue.
gantt-tickets-of-total-in-view = { $count -> [one] { $visible } sur { $count } ticket affiché *[other] { $visible } sur { $count } tickets affichés }
