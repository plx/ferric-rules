(defrule testing
   ?f1<-(orders $?first&:(> (length$ ?first) 0))
   ?f2<-(orders $?others&:(subsetp ?first ?others))
   =>)
(defrule testing
   (orders $?first&:(implode$ ?first)
                   :(implode$ ?first))
   =>)
