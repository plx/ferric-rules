(deftemplate item (slot end))
(deffacts seed (item (end 2)))
(defrule run =>
  (do-for-all-facts ((?f item)) TRUE
    (loop-for-count ?f:end do (printout t "slot" crlf)))
  (printout t "after" crlf))
