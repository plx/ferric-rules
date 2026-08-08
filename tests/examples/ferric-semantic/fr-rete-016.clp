; FR-RETE-016: focus A must be visible to the immediately following RHS
; list-focus-stack action, not deferred until the activation completes.
(defmodule A)

(defrule A::worker
  =>
  (printout t "worker" crlf))

(defrule MAIN::controller
  =>
  (printout t "controller" crlf)
  (focus A)
  (list-focus-stack))
