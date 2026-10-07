(defglobal ?*calls* = 0)
(deftemplate seen (slot n))
(deffunction fields ()
 (bind ?*calls* (+ ?*calls* 1))
 (assert (seen (n ?*calls*)))
 (create$ 2 3))
(defrule run =>
 (printout t (+ 1 (expand$ (fields))) ":" ?*calls* ":"
   (length$ (find-all-facts ((?f seen)) TRUE)) crlf))
