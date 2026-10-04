(deftemplate item (slot n))
(deffacts seed (item (n 1)) (item (n 2)))
(defrule run =>
  (loop-for-count (?i 1 2) do
    (do-for-all-facts ((?f item)) TRUE
      (printout t ?i ":" ?f:n crlf) (break))
    (printout t "outer:" ?i crlf))
  (printout t "after" crlf))
