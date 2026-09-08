;; Issue #326: query traversal follows fact assertion order.
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)))
(defrule probe =>
  (assert (item (value 10)))
  (set-fact-duplication TRUE)
  (assert (item (value 10)))
  (set-fact-duplication FALSE)
  (assert (item (value 10)))
  (do-for-all-facts ((?f item)) TRUE
    (printout t (fact-slot-value ?f value) crlf)))
