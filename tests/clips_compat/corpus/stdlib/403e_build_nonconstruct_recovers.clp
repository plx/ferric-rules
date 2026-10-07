(defrule run =>
  (printout t (build "(assert (unused))") ":" (build word) ":"
    (build "(deftemplate p (slot n (default 7)))") ":"
    (fact-slot-value (assert-string "(p)") n) crlf))
