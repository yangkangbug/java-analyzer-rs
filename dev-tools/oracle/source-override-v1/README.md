# Source same-signature override oracle

Independent compiler declarations and exact native candidates in the existing
semantic runner. Six newly selected calls plus a preserved Parent-static-receiver
control cover multilevel source overrides, distinct-signature overloads, true
source covariant returns, identical returns and a final leaf method. The parent
static receiver never guesses a runtime Child. Some valid declarations carry
@Override, but that annotation is not proof of legality.

Interface/generic/throws/native/array cases remain explicit exact refusals.
Compiler-invalid final, return, static, visibility, unknown and missing-parent
cases cannot pass as empty results. No fixture program is executed.

The frozen prior-update forecast changes only inherited-source override_refusal
in the current stage after candidate verification; original contracts and
--baseline behavior remain intact. No other old positive may become a refusal.
