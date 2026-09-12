;; An action query evaluates its predicate against each bound fact.
;; Level: boundary
;; Covers: queries, do-for-all-facts-filter
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defglobal ?*count* = 0)
(defrule probe =>
    (do-for-all-facts ((?f item)) (> ?f:value 10) (bind ?*count* (+ ?*count* 1)))
    (printout t ?*count* crlf))
