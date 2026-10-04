(deftemplate p (slot x))
; Keep assertion timing the same when the harness installs rules after reset.
(defrule seed (declare (salience 100)) => (assert (go)) (assert (p (x 1))))
(defrule r (go) (test (any-factp ((?f p)) TRUE)) => (printout t fired crlf))
(defrule done (declare (salience -100)) => (printout t done crlf))
