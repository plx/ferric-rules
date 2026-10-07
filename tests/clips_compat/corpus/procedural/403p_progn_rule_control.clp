(defrule run =>
 (loop-for-count (?i 1 3) do
   (progn (printout t ?i) (break) (printout t "bad")))
 (progn (printout t "done" crlf) (return 7) (printout t "bad"))
 (printout t "bad"))
