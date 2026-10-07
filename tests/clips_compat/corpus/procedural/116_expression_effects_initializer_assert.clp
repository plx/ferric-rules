(deffacts seed (p (assert (q 1))))
(defrule run ?holder <- (p ?ref) =>
  (printout t ?ref ":" (fact-slot-value ?ref implied) crlf)
  (printout t ?holder crlf))
