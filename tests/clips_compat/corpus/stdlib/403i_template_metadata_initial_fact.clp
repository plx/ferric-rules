;; initial-fact is a deftemplate without slots, not an implied relation, so
;; a per-slot query of `implied` is an error that stops the rule.
(defrule probe =>
  (printout t (deftemplate-slot-names initial-fact) " "
    (deftemplate-slot-existp initial-fact implied) " "
    (deftemplate-slot-names go) crlf)
  (printout t (deftemplate-slot-multip initial-fact implied) crlf)
  (printout t "not reached" crlf))
(defrule declare-go (go) =>)
