// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package invalid; class Ref {} class Target { private void /*D:invalid*/exact(java.lang.String s) {} } class Use { void run(Target t) { t./*Q:invalid_private*/exact("a"); } }
