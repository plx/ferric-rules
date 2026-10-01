(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (marker))
(defrule probe ?f <- (marker) => (do-for-fact ((?f item)) TRUE (printout t "query:" ?f:value crlf) (retract ?f)) (printout t "outer:" (fact-existp ?f) crlf "remaining:" (length$ (find-all-facts ((?g item)) TRUE)) crlf))
