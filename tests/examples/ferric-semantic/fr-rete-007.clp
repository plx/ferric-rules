; FR-RETE-007 primary stage: a valid rule must survive an adjacent bad rule.
(deffacts startup
  (trigger))

(defrule survivor
  (trigger)
  =>
  (printout t "survivor" crlf)
  (assert (result survivor)))
