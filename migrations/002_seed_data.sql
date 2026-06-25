-- Migration: Seed RBAC data
-- Default roles, permissions, and admin user.

-- ============================================================
-- Roles
-- ============================================================
INSERT INTO roles (id, name, description, created_at)
VALUES
    (uuidv7(), 'system_admin', '系统管理员 — 所有权限', now()),
    (uuidv7(), 'admin', '管理员 — 可管理普通用户并拥有普通用户权限', now()),
    (uuidv7(), 'user', '普通用户 — 基础权限', now());

-- ============================================================
-- Permissions (resource: user)
-- ============================================================
INSERT INTO permissions (id, name, resource, action, description)
VALUES
    (uuidv7(), '读取个人信息', 'user', 'read', '读取自己的用户信息'),
    (uuidv7(), '更新个人信息', 'user', 'update', '更新自己的用户信息'),
    (uuidv7(), '查询用户列表', 'user', 'list', '查询所有用户列表及详情'),
    (uuidv7(), '创建用户', 'user', 'create', '创建新用户'),
    (uuidv7(), '管理用户', 'user', 'manage', '更新和删除其他用户');

-- ============================================================
-- Role ↔ Permission assignments
-- ============================================================

-- Helper: get role and permission IDs by name (uses correlated subqueries)

-- system_admin: all permissions
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.name = 'system_admin';

-- admin: user:list, user:create, user:manage, user:read, user:update
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.name = 'admin'
  AND p.action IN ('list', 'create', 'manage', 'read', 'update');

-- user: user:read, user:update only
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.name = 'user'
  AND p.action IN ('read', 'update');

-- ============================================================
-- Seed admin user (username: admin, password: admin123!@#)
-- ============================================================
INSERT INTO users (id, display_name, username, password_hash, email, phone)
VALUES (
    uuidv7(),
    '系统管理员',
    'admin',
    '$argon2id$v=19$m=19456,t=2,p=1$h0tZ+nuyfZmxbYcayLwbdQ$F2t4kL1um/TF9eXM5nqgIZmwGEYN5Gr1i1KCALTPDEY',
    'admin@example.com',
    '000-00000000'
);

-- Assign system_admin role to the seed admin user
INSERT INTO user_roles (user_id, role_id)
SELECT u.id, r.id
FROM users u, roles r
WHERE u.username = 'admin'
  AND r.name = 'system_admin';
