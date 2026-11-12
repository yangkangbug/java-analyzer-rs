// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package invalid;
class Factory { public Missing make() { return null; } }
class Use { void run(Factory f) { f.make()./*Q:invalid_unknown*/value(); } }
