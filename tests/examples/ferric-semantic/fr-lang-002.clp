; FR-LANG-002: the failing action stops this RHS and the engine; no later
; action or lower-salience activation may produce a sentinel side effect.
(deffacts startup
  (input nope))

(defrule failing-rhs
  (declare (salience 10))
  (input ?value)
  =>
  (+ ?value 1)
  (printout t "AFTER-ACTION" crlf)
  (assert (sentinel after-action)))

(defrule later-activation
  =>
  (printout t "LATER-RULE" crlf)
  (assert (sentinel later-rule)))
