;; find-fact returns the earliest asserted matching fact.
;; Level: boundary
;; Covers: queries, find-fact-first
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
    (bind ?found (find-fact ((?f item)) TRUE))
    (printout t (length$ ?found) ":" (fact-slot-value (nth$ 1 ?found) value) crlf))
