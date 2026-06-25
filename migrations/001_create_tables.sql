-- Migration: Create RBAC tables
-- Tables: users, roles, permissions, role_permissions, user_roles
-- Uses PostgreSQL 18 native uuidv7() for primary keys.

-- ============================================================
-- Users
-- ============================================================
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
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
    deleted_by UUID               -- who performed the soft delete
);

-- Unique constraints (only for non-deleted rows)
CREATE UNIQUE INDEX idx_users_username ON users (username) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX idx_users_email ON users (email) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX idx_users_phone ON users (phone) WHERE deleted_at IS NULL;

-- Keyset pagination index: (created_at DESC, id DESC)
-- Includes WHERE deleted_at IS NULL so list queries are fast.
CREATE INDEX idx_users_cursor ON users (created_at DESC, id DESC) WHERE deleted_at IS NULL;

-- ============================================================
-- Roles
-- ============================================================
CREATE TABLE roles (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    name VARCHAR(50) NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID
);

CREATE UNIQUE INDEX idx_roles_name ON roles (name);

-- ============================================================
-- Permissions
-- ============================================================
CREATE TABLE permissions (
    id UUID PRIMARY KEY DEFAULT uuidv7(),
    name VARCHAR(100) NOT NULL,
    resource VARCHAR(50) NOT NULL,
    action VARCHAR(50) NOT NULL,
    description TEXT
);

CREATE UNIQUE INDEX idx_permissions_resource_action ON permissions (resource, action);

-- ============================================================
-- Role ↔ Permission (many-to-many)
-- ============================================================
CREATE TABLE role_permissions (
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id UUID NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);

-- ============================================================
-- User ↔ Role (many-to-many)
-- ============================================================
CREATE TABLE user_roles (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);
