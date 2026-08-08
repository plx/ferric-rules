; FR-RETE-018 primary stage: reset happens before the late deffacts definition.
(defrule baseline
  =>
  (printout t "baseline" crlf)
  (assert (result baseline)))

(defrule observe-late
  (late)
  =>
  (printout t "late" crlf)
  (assert (result late)))
