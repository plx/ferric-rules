(deffacts d (item 1) (item 2 3))
(defrule query =>
(printout t "any=" (any-factp ((?f item)) (> (length$ ?f:implied) 1)) crlf)
(printout t "find=" (find-fact ((?f item)) TRUE) crlf)
(printout t "all=" (find-all-facts ((?f item)) TRUE) crlf)
(do-for-fact ((?f item)) TRUE (printout t "one=" ?f:implied crlf))
(do-for-all-facts ((?f item)) TRUE (printout t "every=" ?f:implied crlf))
(delayed-do-for-all-facts ((?f item)) TRUE (printout t "delayed=" ?f:implied crlf))
)
