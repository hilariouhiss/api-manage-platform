-- Migration: Create RBAC tables
-- Tables: users, roles, permissions, role_permissions, user_roles
-- Uses PostgreSQL 18 native uuidv7() for primary keys.

-- ============================================================
-- Users
-- ============================================================
CREATE TABLE users (
    id UUID NOT NULL DEFAULT uuidv7(),
    display_name VARCHAR(100) NOT NULL,
    username VARCHAR(50) NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    email VARCHAR(255) NOT NULL,
    phone VARCHAR(30) NOT NULL,
    avatar_url TEXT,
    self_intro TEXT,

    -- Audit fields
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID,
    updated_at TIMESTAMPTZ,
    updated_by UUID,
    deleted_at TIMESTAMPTZ,       -- soft delete
    deleted_by UUID,              -- who performed the soft delete

    CONSTRAINT pk_users PRIMARY KEY (id)
);

-- Unique constraints (only for non-deleted rows)
CREATE UNIQUE INDEX uk_users_username ON users (username) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX uk_users_email ON users (email) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX uk_users_phone ON users (phone) WHERE deleted_at IS NULL;

-- Keyset pagination index: (created_at DESC, id DESC)
-- Includes WHERE deleted_at IS NULL so list queries are fast.
CREATE INDEX idx_users_cursor ON users (created_at DESC, id DESC) WHERE deleted_at IS NULL;

-- ============================================================
-- Roles
-- ============================================================
CREATE TABLE roles (
    id UUID NOT NULL DEFAULT uuidv7(),
    name VARCHAR(50) NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID,

    CONSTRAINT pk_roles PRIMARY KEY (id)
);

CREATE UNIQUE INDEX uk_roles_name ON roles (name);

-- ============================================================
-- Permissions
-- ============================================================
CREATE TABLE permissions (
    id UUID NOT NULL DEFAULT uuidv7(),
    name VARCHAR(100) NOT NULL,
    resource VARCHAR(50) NOT NULL,
    action VARCHAR(50) NOT NULL,
    description TEXT,

    CONSTRAINT pk_permissions PRIMARY KEY (id)
);

CREATE UNIQUE INDEX uk_permissions_resource_action ON permissions (resource, action);

-- ============================================================
-- Role ↔ Permission (many-to-many)
-- ============================================================
CREATE TABLE role_permissions (
    role_id UUID NOT NULL,
    permission_id UUID NOT NULL,

    CONSTRAINT pk_role_permissions PRIMARY KEY (role_id, permission_id),
    CONSTRAINT fk_role_permissions_role FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE,
    CONSTRAINT fk_role_permissions_permission FOREIGN KEY (permission_id) REFERENCES permissions(id) ON DELETE CASCADE
);

-- ============================================================
-- User ↔ Role (many-to-many)
-- ============================================================
CREATE TABLE user_roles (
    user_id UUID NOT NULL,
    role_id UUID NOT NULL,

    CONSTRAINT pk_user_roles PRIMARY KEY (user_id, role_id),
    CONSTRAINT fk_user_roles_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    CONSTRAINT fk_user_roles_role FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE
);
