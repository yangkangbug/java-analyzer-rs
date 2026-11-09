// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package bad;
class Root {}
class Other {}
class Value extends Root { public Value() throws Exception {} }
class Target { public void /*D:invalid_root*/pick(Root x) {} public void /*D:invalid_string*/pick(Other x) {} }
class Use { void run(Target t) { t./*Q:constructor_invalid_throws*/pick(new Value()); } }
