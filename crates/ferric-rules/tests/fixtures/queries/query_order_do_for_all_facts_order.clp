;; Fact query traversal follows assertion order.
;; Level: boundary
;; Covers: queries, do-for-all-facts-order
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
    (do-for-all-facts ((?f item)) TRUE (printout t (fact-slot-value ?f value) crlf)))
