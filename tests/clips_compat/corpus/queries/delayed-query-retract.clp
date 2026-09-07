;; A delayed all-facts query can retract every selected fact.
;; Level: interaction
;; Covers: queries, delayed-query-retract
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defglobal ?*count* = 0)
(defrule probe =>
    (delayed-do-for-all-facts ((?f item)) TRUE
        (retract ?f)
        (bind ?*count* (+ ?*count* 1)))
    (printout t ?*count* crlf))
