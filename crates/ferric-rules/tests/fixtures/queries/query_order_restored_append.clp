;; Issue #326: query traversal follows fact assertion order.
;; Reset, host-retract item10, host-assert item5, then snapshot/restore.
;; Assert item1 after restoration; run, reset, and run again.
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
  (do-for-all-facts ((?f item)) TRUE
    (printout t (fact-slot-value ?f value) crlf))
  (do-for-fact ((?f item)) TRUE
    (printout t "first:" (fact-slot-value ?f value) crlf)))
