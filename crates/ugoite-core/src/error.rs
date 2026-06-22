use thiserror::Error;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorKind {
    InvalidInput,
    Forbidden,
    NotFound,
    Conflict,
    Expired,
    Unimplemented,
    DependencyUnavailable,
    Internal,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorCode {
    InvalidIdentifier,
    Forbidden,
    SpaceAlreadyExists,
    SpaceNotFound,
    FormNotFound,
    EntryNotFound,
    RevisionNotFound,
    RevisionConflict,
    AssetNotFound,
    InvitationExpired,
    InvitationNotFound,
    InvitationNotPending,
    MemberAlreadyActive,
    MemberNotFound,
    LastAdminRequired,
    AssetReferenced,
    SqlSessionExpired,
    ReindexNotImplemented,
    StorageConnectionFailed,
    InvalidInput,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidIdentifier => "INVALID_IDENTIFIER",
            Self::Forbidden => "FORBIDDEN",
            Self::SpaceAlreadyExists => "SPACE_ALREADY_EXISTS",
            Self::SpaceNotFound => "SPACE_NOT_FOUND",
            Self::FormNotFound => "FORM_NOT_FOUND",
            Self::EntryNotFound => "ENTRY_NOT_FOUND",
            Self::RevisionNotFound => "REVISION_NOT_FOUND",
            Self::RevisionConflict => "REVISION_CONFLICT",
            Self::AssetNotFound => "ASSET_NOT_FOUND",
            Self::InvitationExpired => "INVITATION_EXPIRED",
            Self::InvitationNotFound => "INVITATION_NOT_FOUND",
            Self::InvitationNotPending => "INVITATION_NOT_PENDING",
            Self::MemberAlreadyActive => "MEMBER_ALREADY_ACTIVE",
            Self::MemberNotFound => "MEMBER_NOT_FOUND",
            Self::LastAdminRequired => "LAST_ADMIN_REQUIRED",
            Self::AssetReferenced => "ASSET_REFERENCED",
            Self::SqlSessionExpired => "SQL_SESSION_EXPIRED",
            Self::ReindexNotImplemented => "REINDEX_NOT_IMPLEMENTED",
            Self::StorageConnectionFailed => "STORAGE_CONNECTION_FAILED",
            Self::InvalidInput => "INVALID_INPUT",
        }
    }
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct AppError {
    kind: ErrorKind,
    code: ErrorCode,
    message: String,
}

impl AppError {
    pub fn new(kind: ErrorKind, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
        }
    }

    pub fn invalid_identifier(message: impl Into<String>) -> Self {
        Self::new(
            ErrorKind::InvalidInput,
            ErrorCode::InvalidIdentifier,
            message,
        )
    }

    pub fn invalid_input(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, code, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Forbidden, ErrorCode::Forbidden, message)
    }

    pub fn not_found(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, code, message)
    }

    pub fn conflict(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Conflict, code, message)
    }

    pub fn expired(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Expired, code, message)
    }

    pub fn unimplemented(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unimplemented, code, message)
    }

    pub fn dependency_unavailable(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::DependencyUnavailable, code, message)
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn code_str(&self) -> &'static str {
        self.code.as_str()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}
