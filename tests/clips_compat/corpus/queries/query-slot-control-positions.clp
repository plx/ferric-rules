(deftemplate item (slot enabled) (slot stopped) (slot count) (slot key) (multislot parts))
(deffacts seed (item (enabled TRUE) (stopped FALSE) (count 2) (key b) (parts a b)))
(defrule probe =>
  (do-for-all-facts ((?f item)) ?f:enabled
    (if ?f:enabled then (printout t "if" crlf))
    (while ?f:stopped (printout t "unexpected" crlf))
    (loop-for-count (?i ?f:count ?f:count) (printout t "loop:" ?i crlf))
    (switch ?f:key
      (case a then (printout t "unexpected" crlf))
      (case ?f:key then (printout t "case:" ?f:key crlf)))
    (progn$ (?part ?f:parts) (printout t "part:" ?part crlf))
    (foreach ?part ?f:parts (printout t "each:" ?part crlf))))
