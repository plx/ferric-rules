; FR-RETE-017: an unlimited run drains every exhausted focus entry.
(deffacts startup
  (seed))

(defrule consume
  ?seed <- (seed)
  =>
  (retract ?seed)
  (assert (result drained))
  (printout t "drained" crlf))
