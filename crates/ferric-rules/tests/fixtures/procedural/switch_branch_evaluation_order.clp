
(defglobal ?*trace* = 0)
(deffunction mark (?n) (bind ?*trace* (+ (* ?*trace* 10) ?n)) ?n)
(defrule probe =>
 (switch (mark 2)
  (case (mark 1) then (printout t wrong crlf))
  (case (mark 2) then (printout t selected crlf))
  (case (mark 3) then (printout t wrong crlf))
  (default (printout t wrong crlf)))
 (printout t ?*trace* crlf))
