// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package invalid; class Ref {} class Target { public void /*D:invalid*/exact(java.lang.String s) {} public void /*D:invalid_ref*/exact(Ref s) {} } class Use { void run(Target t) { t./*Q:invalid_ambiguous*/exact(null); } }
