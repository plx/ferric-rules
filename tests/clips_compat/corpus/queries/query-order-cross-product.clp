;; Issue #326: query traversal follows fact assertion order.
(deftemplate left-item (slot value))
(deftemplate right-item (slot value))
(deffacts seed
  (right-item (value x)) (left-item (value a))
  (right-item (value y)) (left-item (value b)))
(defrule probe =>
  (do-for-all-facts ((?a left-item) (?b right-item)) TRUE
    (printout t (fact-slot-value ?a value) ":" (fact-slot-value ?b value) crlf))
  (do-for-all-facts ((?b right-item) (?a left-item)) TRUE
    (printout t "reverse:" (fact-slot-value ?b value) ":" (fact-slot-value ?a value) crlf)))
