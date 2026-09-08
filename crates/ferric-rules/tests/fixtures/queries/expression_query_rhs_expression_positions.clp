
(deftemplate item (slot value))
(deftemplate summary (slot total))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
  (if (any-factp ((?f item)) (= ?f:value 20)) then
    (printout t "if:" (not (any-factp ((?f item)) (= ?f:value 99))) crlf))
  (printout t "arith:" (+ 7 (length$ (find-all-facts ((?f item)) (>= ?f:value 20)))) crlf)
  (assert (summary (total (length$ (find-all-facts ((?f item)) TRUE)))))
  (printout t "assert:"
    (fact-slot-value (nth$ 1 (find-fact ((?s summary)) TRUE)) total) crlf))
