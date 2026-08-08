; FR-RETE-015 primary stage: module A deliberately omits exports.
(defrule MAIN::baseline
  =>
  (printout t "baseline" crlf))

(defmodule A)

(deftemplate A::secret
  (slot value))
