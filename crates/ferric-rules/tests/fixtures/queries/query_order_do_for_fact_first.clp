;; do-for-fact executes its body once, for the earliest matching fact.
;; Level: boundary
;; Covers: queries, do-for-fact-first
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
    (do-for-fact ((?f item)) TRUE (printout t (fact-slot-value ?f value) crlf)))
