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
admin-api-tokens-revoke-confirm-prefix = Confirmez-vous la révocation du jeton
admin-api-tokens-revoke-confirm-suffix = ?
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
