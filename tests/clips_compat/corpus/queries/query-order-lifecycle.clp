;; Issue #326: query traversal follows fact assertion order after a retraction
;; and a later assertion; each reset restores the deffacts order first.
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule replace-first (declare (salience 10))
  ?f <- (item (value 10))
  =>
  (retract ?f)
  (assert (item (value 5))))
(defrule probe =>
  (do-for-all-facts ((?f item)) TRUE
    (printout t (fact-slot-value ?f value) crlf))
  (do-for-fact ((?f item)) TRUE
    (printout t "first:" (fact-slot-value ?f value) crlf)))
