(deftemplate item (slot n))
(deffacts seed (item (n 1)) (item (n 2)) (item (n 3)))
(defrule run =>
  (do-for-fact ((?f item)) TRUE
    (printout t "first:" ?f:n crlf) (break) (printout t "unexpected" crlf))
  (do-for-all-facts ((?f item)) TRUE
    (printout t "all:" ?f:n crlf) (break) (printout t "unexpected" crlf))
  (delayed-do-for-all-facts ((?f item)) TRUE
    (printout t "delayed:" ?f:n crlf) (break) (printout t "unexpected" crlf))
  (printout t "after" crlf))
