(deffacts list-fact                ; DR0508
   (list 12 "=" 3.0 i2))
(defrule test-member               ; DR0508
   (list $?list)
   =>
   (printout t "position=" (member i2 ?list) crlf))
